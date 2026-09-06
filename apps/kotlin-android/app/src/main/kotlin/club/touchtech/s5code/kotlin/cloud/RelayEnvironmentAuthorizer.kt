package club.touchtech.s5code.kotlin.cloud

import club.touchtech.s5code.kotlin.data.SavedEnvironment
import club.touchtech.s5code.kotlin.transport.EnvironmentAuthorizer
import club.touchtech.s5code.kotlin.transport.EnvironmentCredential
import club.touchtech.s5code.kotlin.transport.EnvironmentHttp
import club.touchtech.s5code.kotlin.transport.EnvironmentHttpError
import club.touchtech.s5code.kotlin.transport.EnvironmentHttpErrorKind

/**
 * Authorization for a relay-managed environment, mirroring
 * `connectRelayManagedEnvironment` in `apps/mobile/src/features/cloud/linkEnvironment.ts`.
 *
 * Four steps, in this order, each of which can fail in a way the user can act on:
 *
 * 1. ask the relay to mint a pairing credential bound to this device's key;
 * 2. confirm the endpoint it returned really is the environment we asked for, so
 *    a relay bug or a hostname reuse cannot silently point the app at another
 *    machine;
 * 3. exchange the credential for a DPoP-bound access token at the environment;
 * 4. hand back that token plus the endpoint the relay named.
 *
 * Every connection repeats all four. The credential is single-use and short-lived
 * by design, so there is nothing here worth caching, and the descriptor check is
 * cheap next to opening a socket.
 */
class RelayEnvironmentAuthorizer(
    private val relay: RelayClient,
    private val http: EnvironmentHttp,
    private val key: DpopKey,
    private val deviceId: suspend () -> String?,
    private val deviceLabel: String,
    private val onEndpointResolved: suspend (environmentId: String, httpBaseUrl: String, wsBaseUrl: String) -> Unit =
        { _, _, _ -> },
) : EnvironmentAuthorizer {

    override suspend fun authorize(environment: SavedEnvironment): EnvironmentAuthorizer.Authorized {
        val connect =
            try {
                relay.connectEnvironment(environment.environmentId, deviceId())
            } catch (error: RelayError) {
                throw EnvironmentHttpError(
                    if (error.unauthorized) EnvironmentHttpErrorKind.Unauthorized
                    else EnvironmentHttpErrorKind.Unreachable,
                    error.message,
                    error,
                )
            }
        if (connect.environmentId != environment.environmentId) {
            throw EnvironmentHttpError(
                EnvironmentHttpErrorKind.Protocol,
                "The relay returned credentials for a different machine.",
            )
        }

        val descriptor = http.descriptor(connect.endpoint.httpBaseUrl)
        if (descriptor.environmentId != connect.environmentId) {
            throw EnvironmentHttpError(
                EnvironmentHttpErrorKind.Protocol,
                "That tunnel is serving a different machine than the relay reported.",
            )
        }

        val access =
            http.exchangeProofBoundCredential(
                httpBaseUrl = connect.endpoint.httpBaseUrl,
                credential = connect.credential,
                deviceLabel = deviceLabel,
                proof = { url -> key.createProof("POST", url) },
            )

        // The tunnel hostname can change between links, so the saved row follows
        // what the relay just said rather than staying on a dead host.
        onEndpointResolved(
            environment.environmentId,
            connect.endpoint.httpBaseUrl,
            connect.endpoint.wsBaseUrl,
        )

        return EnvironmentAuthorizer.Authorized(
            httpBaseUrl = connect.endpoint.httpBaseUrl,
            wsBaseUrl = connect.endpoint.wsBaseUrl,
            credential =
                EnvironmentCredential.Dpop(access.access_token) { method, url, accessToken ->
                    key.createProof(method, url, accessToken)
                },
        )
    }
}
