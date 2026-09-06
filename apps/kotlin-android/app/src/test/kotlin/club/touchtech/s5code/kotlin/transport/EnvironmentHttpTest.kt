package club.touchtech.s5code.kotlin.transport

import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.OkHttpClient
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * The environment HTTP contract, over a real socket.
 *
 * Every assertion here is a method, header, or URL shape the server enforces and
 * a fixture test cannot see. The session verb is pinned because it was wrong: the
 * contract defines `/api/auth/session` as a `GET`, the client sent `POST`, and the
 * server's 404 surfaced as "that address answered, but not like an S5 Code
 * server" — a message that blames the address for a client bug.
 */
class EnvironmentHttpTest {

    private val server = MockWebServer()
    private val http = EnvironmentHttp(OkHttpClient())

    @Before fun setUp() = server.start()

    @After fun tearDown() = server.close()

    private fun json(body: String, code: Int = 200) =
        MockResponse.Builder()
            .code(code)
            .setHeader("content-type", "application/json")
            .body(body)
            .build()

    private fun base() = server.url("/").toString().trimEnd('/')

    @Test
    fun `session is a GET with no body`() = runTest {
        server.enqueue(json("""{"authenticated":true,"auth":{"mode":"token"}}"""))

        assertTrue(http.session(base(), EnvironmentCredential.Bearer("token-1")).authenticated)

        val request = server.takeRequest()
        assertEquals("GET", request.method)
        assertEquals("/api/auth/session", request.url.encodedPath)
        assertEquals("Bearer token-1", request.headers["authorization"])
        assertEquals(0L, request.body?.size ?: 0L)
    }

    @Test
    fun `websocket ticket is a POST`() = runTest {
        server.enqueue(json("""{"ticket":"tkt","expiresAt":"2026-01-01T00:00:00Z"}"""))

        assertEquals("tkt", http.webSocketTicket(base(), EnvironmentCredential.Bearer("t")).ticket)

        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertEquals("/api/auth/websocket-ticket", request.url.encodedPath)
    }

    @Test
    fun `a dpop credential signs the proof over the method it actually sends`() = runTest {
        server.enqueue(json("""{"authenticated":true,"auth":{"mode":"token"}}"""))
        var signedMethod: String? = null
        var signedUrl: String? = null

        http.session(
            base(),
            EnvironmentCredential.Dpop("access-1") { method, url, accessToken ->
                signedMethod = method
                signedUrl = url
                "proof-for-$method-$accessToken"
            },
        )

        // The proof binds the method; signing "POST" for a GET is rejected as a
        // method mismatch even though the signature itself is valid.
        assertEquals("GET", signedMethod)
        assertTrue(signedUrl!!.endsWith("/api/auth/session"))
        val request = server.takeRequest()
        assertEquals("DPoP access-1", request.headers["authorization"])
        assertEquals("proof-for-GET-access-1", request.headers["dpop"])
    }

    @Test
    fun `the descriptor decodes the platform struct`() = runTest {
        server.enqueue(
            json(
                """{"environmentId":"env-1","label":"Desk","platform":{"os":"linux","arch":"arm64"},"serverVersion":"0.2.11","capabilities":{"connectionProbe":true}}"""
            )
        )

        val descriptor = http.descriptor(base())

        assertEquals("linux", descriptor.platform.os)
        assertEquals("arm64", descriptor.platform.arch)
        assertEquals("linux/arm64", descriptor.platform.display)
        assertTrue(descriptor.capabilities.connectionProbe)
        assertEquals("GET", server.takeRequest().method)
    }

    @Test
    fun `an unknown capability does not fail the descriptor`() = runTest {
        // Capability flags grow every release; one this build has never heard of
        // must not take down pairing.
        server.enqueue(
            json(
                """{"environmentId":"env-1","label":"Desk","platform":{"os":"darwin","arch":"other"},"serverVersion":"9.9.9","capabilities":{"timeTravel":true,"threadPinning":true}}"""
            )
        )

        val descriptor = http.descriptor(base())

        assertTrue(descriptor.capabilities.threadPinning)
        assertEquals("darwin/other", descriptor.platform.display)
    }

    @Test
    fun `socket url appends the ticket and only adds ws when the base has no path`() {
        assertEquals(
            "wss://host.example/ws?wsTicket=a%2Fb",
            EnvironmentHttp.socketUrl("wss://host.example", "a/b"),
        )
        assertEquals(
            "wss://host.example/ws?wsTicket=t",
            EnvironmentHttp.socketUrl("wss://host.example/", "t"),
        )
        // A relay tunnel can hand back a URL that already routes. Overwriting that
        // path sends the upgrade somewhere the server is not listening.
        assertEquals(
            "wss://tunnel.example/env/abc?wsTicket=t",
            EnvironmentHttp.socketUrl("wss://tunnel.example/env/abc", "t"),
        )
    }

    @Test
    fun `a spent pairing token reads as unauthorized, not as a wrong address`() = runTest {
        server.enqueue(json("""{"error":"invalid_grant"}""", code = 401))

        val error =
            runCatching {
                    http.exchangePairingCredential(base(), "spent", "Pixel")
                }
                .exceptionOrNull() as EnvironmentHttpError

        assertEquals(EnvironmentHttpErrorKind.Unauthorized, error.kind)
        assertTrue(error.message.contains("already been used"))
    }

    @Test
    fun `a 404 reads as the wrong address`() = runTest {
        server.enqueue(json("""{}""", code = 404))

        val error =
            runCatching { http.descriptor(base()) }.exceptionOrNull() as EnvironmentHttpError

        assertEquals(EnvironmentHttpErrorKind.Protocol, error.kind)
    }

    @Test
    fun `the token exchange sends the form the contract requires`() = runTest {
        server.enqueue(
            json(
                """{"access_token":"acc","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read"}"""
            )
        )

        http.exchangePairingCredential(base(), "one-time", "Pixel 9")

        val form = server.takeRequest().body?.utf8().orEmpty()
        assertTrue(form.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange"))
        assertTrue(form.contains("subject_token=one-time"))
        assertTrue(form.contains("subject_token_type=urn%3At3%3Aparams%3Aoauth%3Atoken-type%3Aenvironment-bootstrap"))
        assertTrue(form.contains("client_device_type=mobile"))
        assertTrue(form.contains("client_os=Android"))
        // Administrative scopes are deliberately not requested.
        assertTrue(form.contains("orchestration%3Aread"))
        assertTrue("the exchange must not need a second round trip", server.requestCount == 1)
    }
}
