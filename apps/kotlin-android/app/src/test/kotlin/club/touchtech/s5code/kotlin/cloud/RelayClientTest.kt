package club.touchtech.s5code.kotlin.cloud

import java.security.KeyPairGenerator
import java.security.SecureRandom
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.OkHttpClient
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

/**
 * The relay protocol as this client speaks it, against a real socket.
 *
 * Every assertion here is something the relay enforces and a device cannot see
 * locally: the token exchange is form-encoded with an exact `resource`, DPoP calls
 * carry both the `DPoP` authorization and a proof, the access token is cached
 * across calls, and a rejected token is retried exactly once.
 */
class RelayClientTest {

    private val server = MockWebServer()

    @Before fun setUp() = server.start()

    @After fun tearDown() = server.close()

    private fun client(): RelayClient {
        val generator = KeyPairGenerator.getInstance("EC")
        generator.initialize(ECGenParameterSpec("secp256r1"), SecureRandom())
        val pair = generator.generateKeyPair()
        return RelayClient(
            relayUrl = server.url("/").toString().trimEnd('/'),
            client = OkHttpClient(),
            key = DpopKey(pair.private, DpopKey.jwkOf(pair.public as ECPublicKey)),
            clerkToken = { "clerk-token" },
        )
    }

    private fun json(body: String, code: Int = 200) =
        MockResponse.Builder()
            .code(code)
            .setHeader("content-type", "application/json")
            .body(body)
            .build()

    private val accessTokenBody =
        """{"access_token":"relay-access","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"DPoP","expires_in":3600,"scope":"environment:connect environment:status"}"""

    private val mobileAccessTokenBody =
        """{"access_token":"relay-mobile","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"DPoP","expires_in":3600,"scope":"mobile:registration"}"""

    @Test
    fun `listing environments uses the clerk bearer token directly`() = runTest {
        server.enqueue(
            json("""{"environments":[{"environmentId":"env-1","label":"Desk","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"linkedAt":"now"}]}""")
        )

        val environments = client().listEnvironments()

        assertEquals(1, environments.size)
        assertEquals("Desk", environments[0].label)
        val request = server.takeRequest()
        assertEquals("/v1/environments", request.url.encodedPath)
        assertEquals("Bearer clerk-token", request.headers["authorization"])
        // A bearer call must not carry a proof: the relay's bearer middleware does
        // not expect one, and sending it is how a client ends up looking like it is
        // trying two auth schemes at once.
        assertEquals(null, request.headers["dpop"])
    }

    @Test
    fun `listing devices uses the clerk bearer token directly`() = runTest {
        server.enqueue(json("""{"devices":[{"deviceId":"phone-1","label":"Pixel","platform":"android","appVersion":"0.1","updatedAt":"now"}]}"""))

        val devices = client().listDevices()

        assertEquals(listOf("Pixel"), devices.map { it.label })
        val request = server.takeRequest()
        assertEquals("/v1/client/devices", request.url.encodedPath)
        assertEquals("Bearer clerk-token", request.headers["authorization"])
    }

    @Test
    fun `connect exchanges a scoped access token then sends proof-bound request`() = runTest {
        server.enqueue(json(accessTokenBody))
        server.enqueue(
            json("""{"environmentId":"env-1","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"credential":"pair-me","expiresAt":"soon"}""")
        )

        val connect = client().connectEnvironment("env-1", deviceId = "device-9")

        assertEquals("pair-me", connect.credential)

        val exchange = server.takeRequest()
        assertEquals("/v1/client/dpop-token", exchange.url.encodedPath)
        val form = exchange.body?.utf8().orEmpty()
        assertTrue(form.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange"))
        assertTrue(form.contains("subject_token=clerk-token"))
        assertTrue(form.contains("client_id=t3-mobile"))
        assertTrue("scope must be space separated", form.contains("scope=environment%3Aconnect"))
        // The relay compares `resource` against its own issuer, which has no
        // trailing slash. A mismatch is a flat rejection.
        assertTrue(form.contains("resource=${server.url("/").toString().trimEnd('/').replace(":", "%3A").replace("/", "%2F")}"))
        // Bootstrap proof only: there is no access token to bind yet.
        val exchangeProof = exchange.headers["dpop"]
        assertTrue(exchangeProof != null && !claims(exchangeProof).contains("\"ath\""))

        val connectRequest = server.takeRequest()
        assertEquals("/v1/environments/env-1/connect", connectRequest.url.encodedPath)
        assertEquals("DPoP relay-access", connectRequest.headers["authorization"])
        val proof = connectRequest.headers["dpop"]
        assertTrue("a token-bound call must carry ath", claims(proof!!).contains("\"ath\""))
        val payload = connectRequest.body?.utf8().orEmpty()
        assertTrue(payload.contains("clientProofKeyThumbprint"))
        assertTrue(payload.contains("device-9"))
    }

    @Test
    fun `the access token is reused across calls in the same scope`() = runTest {
        server.enqueue(json(accessTokenBody))
        server.enqueue(json("""{"environmentId":"env-1","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"status":"online","checkedAt":"now"}"""))
        server.enqueue(json("""{"environmentId":"env-1","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"status":"offline","checkedAt":"now"}"""))

        val relay = client()
        assertEquals("online", relay.environmentStatus("env-1").status)
        assertEquals("offline", relay.environmentStatus("env-1").status)

        assertEquals("/v1/client/dpop-token", server.takeRequest().url.encodedPath)
        assertEquals("/v1/environments/env-1/status", server.takeRequest().url.encodedPath)
        // No second exchange: the cached token still covers this scope.
        assertEquals("/v1/environments/env-1/status", server.takeRequest().url.encodedPath)
    }

    @Test
    fun `a rejected access token is refreshed once`() = runTest {
        server.enqueue(json(accessTokenBody))
        server.enqueue(
            json("""{"_tag":"RelayAuthInvalidError","reason":"invalid_bearer"}""", code = 401)
        )
        server.enqueue(json(accessTokenBody))
        server.enqueue(json("""{"environmentId":"env-1","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"status":"online","checkedAt":"now"}"""))

        assertEquals("online", client().environmentStatus("env-1").status)

        assertEquals("/v1/client/dpop-token", server.takeRequest().url.encodedPath)
        assertEquals("/v1/environments/env-1/status", server.takeRequest().url.encodedPath)
        assertEquals("/v1/client/dpop-token", server.takeRequest().url.encodedPath)
        assertEquals("/v1/environments/env-1/status", server.takeRequest().url.encodedPath)
    }

    @Test
    fun `a missing environment link reports what the user has to fix`() = runTest {
        server.enqueue(json(accessTokenBody))
        server.enqueue(
            json(
                """{"_tag":"RelayEnvironmentConnectNotAuthorizedError","reason":"environment_link_not_found"}""",
                code = 403,
            )
        )
        server.enqueue(json(accessTokenBody))
        server.enqueue(
            json(
                """{"_tag":"RelayEnvironmentConnectNotAuthorizedError","reason":"environment_link_not_found"}""",
                code = 403,
            )
        )

        try {
            client().connectEnvironment("env-1", deviceId = null)
            fail("expected a relay error")
        } catch (error: RelayError) {
            assertTrue(error.message.contains("no active link"))
        }
    }

    @Test
    fun `mobile registration exchanges exact scope and sends Android payload`() = runTest {
        server.enqueue(json(mobileAccessTokenBody))
        server.enqueue(json("""{"ok":true}"""))
        val relay = client()

        relay.registerDevice(
            RelayDeviceRegistrationRequestDto(
                deviceId = "device-9",
                label = "Pixel",
                platform = "android",
                appVersion = "0.1.0",
                fcmToken = "fcm-token",
                preferences =
                    RelayAgentAwarenessPreferencesDto(
                        liveActivitiesEnabled = true,
                        notificationsEnabled = true,
                        notifyOnApproval = true,
                        notifyOnInput = false,
                        notifyOnCompletion = true,
                        notifyOnFailure = false,
                    ),
            )
        )

        val exchange = server.takeRequest()
        assertEquals("/v1/client/dpop-token", exchange.url.encodedPath)
        assertTrue(exchange.body?.utf8().orEmpty().contains("scope=mobile%3Aregistration"))
        val registration = server.takeRequest()
        assertEquals("/v1/mobile/devices", registration.url.encodedPath)
        assertEquals("DPoP relay-mobile", registration.headers["authorization"])
        val payload = registration.body?.utf8().orEmpty()
        assertTrue(payload.contains("\"platform\":\"android\""))
        assertTrue(payload.contains("\"fcmToken\":\"fcm-token\""))
        assertTrue(payload.contains("\"notifyOnInput\":false"))
    }

    @Test
    fun `live update registration uses persisted generation endpoint`() = runTest {
        server.enqueue(json(mobileAccessTokenBody))
        server.enqueue(json("""{"ok":true}"""))

        client().registerAndroidLiveUpdate("device-9", "generation-2")

        server.takeRequest()
        val registration = server.takeRequest()
        assertEquals("/v1/mobile/android-live-updates", registration.url.encodedPath)
        assertEquals(
            "{\"deviceId\":\"device-9\",\"generationId\":\"generation-2\"}",
            registration.body?.utf8(),
        )
    }

    @Test
    fun `signing out drops the cached token`() = runTest {
        server.enqueue(json(accessTokenBody))
        server.enqueue(json("""{"environmentId":"env-1","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"status":"online","checkedAt":"now"}"""))
        server.enqueue(json(accessTokenBody))
        server.enqueue(json("""{"environmentId":"env-1","endpoint":{"httpBaseUrl":"https://a.test","wsBaseUrl":"wss://a.test","providerKind":"cloudflare_tunnel"},"status":"online","checkedAt":"now"}"""))

        val relay = client()
        relay.environmentStatus("env-1")
        relay.reset()
        relay.environmentStatus("env-1")

        assertEquals(4, server.requestCount)
    }

    private fun claims(proof: String): String =
        String(java.util.Base64.getUrlDecoder().decode(proof.split(".")[1]))
}
