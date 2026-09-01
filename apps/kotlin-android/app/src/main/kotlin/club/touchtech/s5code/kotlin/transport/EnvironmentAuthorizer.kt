package club.touchtech.s5code.kotlin.transport

import club.touchtech.s5code.kotlin.data.EnvironmentStore
import club.touchtech.s5code.kotlin.data.SavedEnvironment

/**
 * How a session gets the credential and endpoint for one environment.
 *
 * Direct and relay-managed environments differ in two ways that both have to be
 * resolved at connect time rather than at pair time:
 *
 * - **The credential.** A direct token is long-lived and read from the store. A
 *   relay-managed one is short-lived and minted per connection, bound to this
 *   device's proof key.
 * - **The endpoint.** A managed tunnel's hostname is stable across a link but not
 *   across a re-provision, so the relay's answer wins over whatever was saved.
 *
 * Keeping both behind this interface is what lets [EnvironmentSession] have one
 * connect path: it asks for authorization and opens a socket, with no branch on
 * how the environment was paired.
 */
interface EnvironmentAuthorizer {
    suspend fun authorize(environment: SavedEnvironment): Authorized

    data class Authorized(
        val httpBaseUrl: String,
        val wsBaseUrl: String,
        val credential: EnvironmentCredential,
    )
}

/**
 * Reads the saved bearer token. This is the whole authorization story for a
 * directly paired environment: the token was exchanged once and is reused until
 * the user unpairs.
 */
class DirectEnvironmentAuthorizer(private val store: EnvironmentStore) : EnvironmentAuthorizer {
    override suspend fun authorize(environment: SavedEnvironment): EnvironmentAuthorizer.Authorized {
        val token =
            store.token(environment)
                ?: throw EnvironmentHttpError(
                    EnvironmentHttpErrorKind.Unauthorized,
                    "The saved credential for this environment could not be read.",
                )
        return EnvironmentAuthorizer.Authorized(
            httpBaseUrl = environment.httpBaseUrl,
            wsBaseUrl = environment.wsBaseUrl,
            credential = EnvironmentCredential.Bearer(token),
        )
    }
}
