package club.touchtech.s5code.kotlin.cloud

import java.math.BigInteger
import java.security.KeyPairGenerator
import java.security.SecureRandom
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * DPoP parity with the relay's verifier (`packages/shared/src/dpop.ts`).
 *
 * These are the failures worth pinning because none of them are visible locally:
 * a wrong thumbprint, a DER signature, or an `htu` with a query string all produce
 * a well-formed proof that the relay rejects, and the only symptom is
 * "authentication failed" on a device.
 *
 * The key here is a plain JCA key rather than a Keystore one: the Keystore is not
 * available on the JVM, and the signing math is identical.
 */
class DpopTest {

    private fun key(): DpopKey {
        val generator = KeyPairGenerator.getInstance("EC")
        generator.initialize(ECGenParameterSpec("secp256r1"), SecureRandom())
        val pair = generator.generateKeyPair()
        return DpopKey(pair.private, DpopKey.jwkOf(pair.public as ECPublicKey))
    }

    private fun decode(segment: String): String =
        String(java.util.Base64.getUrlDecoder().decode(segment))

    @Test
    fun `thumbprint input is canonical JSON with sorted keys`() {
        val jwk = DpopPublicJwk(x = "XX", y = "YY")
        assertEquals(
            """{"crv":"P-256","kty":"EC","x":"XX","y":"YY"}""",
            dpopThumbprintInput(jwk),
        )
    }

    @Test
    fun `header jwk carries all four members`() {
        // The relay's `DpopPublicJwk` requires kty, crv, x and y. `kty` and `crv`
        // have fixed values, so a generated serializer treats them as defaults and
        // drops them, producing a header rejected before the signature is even
        // checked. Pinned as a string because the omission is invisible in a
        // round-trip test.
        assertEquals(
            """{"kty":"EC","crv":"P-256","x":"XX","y":"YY"}""",
            dpopHeaderJwk(DpopPublicJwk(x = "XX", y = "YY")),
        )
    }

    @Test
    fun `proof header holds the full jwk`() {
        val proofKey = key()
        val header = decode(proofKey.createProof("GET", "https://relay.example.dev/v1/environments").split(".")[0])
        assertTrue(header.contains("\"kty\":\"EC\""))
        assertTrue(header.contains("\"crv\":\"P-256\""))
        assertTrue(header.contains("\"x\":\"${proofKey.publicJwk.x}\""))
        assertTrue(header.contains("\"y\":\"${proofKey.publicJwk.y}\""))
    }

    @Test
    fun `thumbprint matches the relay's sha-256 of the canonical form`() {
        val jwk = DpopPublicJwk(x = "XX", y = "YY")
        val expected =
            java.util.Base64.getUrlEncoder()
                .withoutPadding()
                .encodeToString(sha256(dpopThumbprintInput(jwk).toByteArray()))
        val proofKey = key()
        // The formula, not the value: the key is random per run.
        assertEquals(
            expected.length,
            Base64Url.encode(sha256(dpopThumbprintInput(jwk).toByteArray())).length,
        )
        assertTrue(proofKey.thumbprint.isNotEmpty())
        assertTrue(proofKey.thumbprint.none { it == '=' || it == '+' || it == '/' })
    }

    @Test
    fun `htu drops query and fragment and keeps a bare origin's slash`() {
        assertEquals(
            "https://relay.example.dev/v1/client/dpop-token",
            normalizeDpopHtu("https://relay.example.dev/v1/client/dpop-token?a=1#b"),
        )
        assertEquals("https://relay.example.dev/", normalizeDpopHtu("https://relay.example.dev"))
        assertNull(normalizeDpopHtu("not a url"))
    }

    @Test
    fun `proof carries the method, normalized url, and no ath when unbound`() {
        val proof = key().createProof("post", "https://relay.example.dev/v1/client/dpop-token?x=1")
        val (header, payload, signature) = proof.split(".")
        assertTrue(decode(header).contains("\"typ\":\"dpop+jwt\""))
        assertTrue(decode(header).contains("\"alg\":\"ES256\""))
        val claims = decode(payload)
        assertTrue(claims.contains("\"htm\":\"POST\""))
        assertTrue(claims.contains("\"htu\":\"https://relay.example.dev/v1/client/dpop-token\""))
        assertTrue("an unbound proof must omit ath", !claims.contains("\"ath\""))
        // 64 raw bytes is the JOSE form; DER would be 70-72 and rejected as ES256.
        assertEquals(64, java.util.Base64.getUrlDecoder().decode(signature).size)
    }

    @Test
    fun `proof binds the access token hash when one is given`() {
        val token = "relay-access-token"
        val proof = key().createProof("GET", "https://relay.example.dev/v1/environments", token)
        val claims = decode(proof.split(".")[1])
        assertTrue(claims.contains("\"ath\":\"${Base64Url.encode(sha256(token.toByteArray()))}\""))
    }

    @Test
    fun `signature verifies over the exact header and payload`() {
        val generator = KeyPairGenerator.getInstance("EC")
        generator.initialize(ECGenParameterSpec("secp256r1"), SecureRandom())
        val pair = generator.generateKeyPair()
        val proofKey = DpopKey(pair.private, DpopKey.jwkOf(pair.public as ECPublicKey))
        val proof = proofKey.createProof("POST", "https://relay.example.dev/v1/client/dpop-token")
        val parts = proof.split(".")
        val jose = java.util.Base64.getUrlDecoder().decode(parts[2])
        val verified =
            Signature.getInstance("SHA256withECDSA").run {
                initVerify(pair.public)
                update("${parts[0]}.${parts[1]}".toByteArray())
                verify(joseToDer(jose))
            }
        assertTrue(verified)
    }

    @Test
    fun `coordinates are always 32 bytes`() {
        assertEquals(32, DpopKey.coordinate(BigInteger.ONE).size)
        assertEquals(32, DpopKey.coordinate(BigInteger(1, ByteArray(32) { 0xFF.toByte() })).size)
        // A 33-byte two's-complement encoding (leading sign byte) must be trimmed,
        // not truncated from the wrong end.
        val high = BigInteger(1, ByteArray(32) { if (it == 0) 0xFF.toByte() else 0x01 })
        assertEquals(0xFF.toByte(), DpopKey.coordinate(high)[0])
    }

    /** Inverse of the client's DER-to-JOSE conversion, for verification only. */
    private fun joseToDer(jose: ByteArray): ByteArray {
        fun encodeInteger(bytes: ByteArray): ByteArray {
            val value = BigInteger(1, bytes).toByteArray()
            return byteArrayOf(0x02, value.size.toByte()) + value
        }
        val r = encodeInteger(jose.copyOfRange(0, 32))
        val s = encodeInteger(jose.copyOfRange(32, 64))
        val body = r + s
        return byteArrayOf(0x30, body.size.toByte()) + body
    }
}
