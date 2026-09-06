package club.touchtech.s5code.kotlin.cloud

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.math.BigInteger
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.util.UUID

/**
 * The public half of the device's relay proof key, in the JWK shape the relay
 * verifies (`DpopPublicJwk` in `packages/shared/src/dpopCommon.ts`).
 *
 * Deliberately not `@Serializable`: `kty` and `crv` have fixed values, so a
 * generated serializer treats them as defaults and omits them, and the relay
 * requires all four members. Both JSON forms this type needs are written by hand
 * below, for the thumbprint and for the proof header.
 */
data class DpopPublicJwk(
    val kty: String = "EC",
    val crv: String = "P-256",
    val x: String,
    val y: String,
)

/**
 * Base64url without padding, which is what every field of a DPoP proof uses. The
 * JDK encoder is configured rather than post-processed so a stray `=` can never
 * reach a signature input.
 */
internal object Base64Url {
    private val encoder = java.util.Base64.getUrlEncoder().withoutPadding()

    fun encode(bytes: ByteArray): String = encoder.encodeToString(bytes)

    fun encode(value: String): String = encode(value.toByteArray())
}

/**
 * Stable JSON in the exact form `stableStringify` produces
 * (`packages/shared/src/relaySigning.ts`): keys sorted, no spaces. The thumbprint
 * is a hash of this string, so a different key order is a different device as far
 * as the relay is concerned.
 */
internal fun dpopThumbprintInput(jwk: DpopPublicJwk): String =
    """{"crv":"${jwk.crv}","kty":"${jwk.kty}","x":"${jwk.x}","y":"${jwk.y}"}"""

/**
 * The JWK as it appears in a proof header.
 *
 * Written by hand rather than serialized. `kty` and `crv` are fixed values, so
 * they read as serializer defaults, and kotlinx.serialization omits defaults —
 * which produced a header the relay rejected outright while the thumbprint
 * (computed from the canonical form above, not from this) still looked correct.
 * All four members are required by `DpopPublicJwk` in
 * `packages/shared/src/dpopCommon.ts`.
 */
internal fun dpopHeaderJwk(jwk: DpopPublicJwk): String =
    """{"kty":"${jwk.kty}","crv":"${jwk.crv}","x":"${jwk.x}","y":"${jwk.y}"}"""

internal fun sha256(bytes: ByteArray): ByteArray =
    java.security.MessageDigest.getInstance("SHA-256").digest(bytes)

/**
 * Normalizes a URL for the `htu` claim, matching `normalizeDpopHtu`: query and
 * fragment are dropped, everything else is preserved. A mismatch here is rejected
 * by the relay as a replayed proof, so the two implementations have to agree
 * character for character.
 */
internal fun normalizeDpopHtu(url: String): String? {
    val parsed = runCatching { java.net.URI(url) }.getOrNull() ?: return null
    val scheme = parsed.scheme ?: return null
    val authority = parsed.authority ?: return null
    val path = parsed.rawPath.orEmpty()
    // `new URL("https://host").toString()` is "https://host/", so a bare origin
    // has to carry the slash the JS side would add.
    val normalizedPath = if (path.isEmpty()) "/" else path
    return "$scheme://$authority$normalizedPath"
}

/**
 * The device's DPoP proof key, held in the Android Keystore.
 *
 * The private key never enters the process: the Keystore signs on the app's
 * behalf, so a compromised heap or a stolen backup cannot reproduce a proof.
 * That is the property the relay is relying on when it binds an access token to
 * this key's thumbprint — the same guarantee `expo-secure-store` gives the React
 * Native client, one level stronger because the key is non-exportable.
 */
class DpopKey internal constructor(
    private val privateKey: PrivateKey,
    val publicJwk: DpopPublicJwk,
) {

    /** Base64url SHA-256 of the canonical JWK, the relay's device identity. */
    val thumbprint: String by lazy { Base64Url.encode(sha256(dpopThumbprintInput(publicJwk).toByteArray())) }

    /**
     * Signs a DPoP proof for one request. [accessToken] is included as `ath` when
     * present, which is required on every call that carries a DPoP-bound token
     * and must be absent on the bootstrap token exchange.
     */
    fun createProof(method: String, url: String, accessToken: String? = null): String {
        val htu = normalizeDpopHtu(url) ?: error("DPoP URL is invalid: $url")
        val header =
            Base64Url.encode(
                """{"typ":"dpop+jwt","alg":"ES256","jwk":${dpopHeaderJwk(publicJwk)}}"""
            )
        val claims = buildString {
            append("{\"htm\":\"").append(method.uppercase()).append("\"")
            append(",\"htu\":\"").append(htu).append("\"")
            append(",\"jti\":\"").append(UUID.randomUUID().toString()).append("\"")
            append(",\"iat\":").append(System.currentTimeMillis() / 1_000)
            if (accessToken != null) {
                append(",\"ath\":\"").append(Base64Url.encode(sha256(accessToken.toByteArray()))).append("\"")
            }
            append("}")
        }
        val payload = Base64Url.encode(claims)
        val signingInput = "$header.$payload"
        val signature =
            Signature.getInstance("SHA256withECDSA").run {
                initSign(privateKey)
                update(signingInput.toByteArray())
                sign()
            }
        return "$signingInput.${Base64Url.encode(derToJoseSignature(signature))}"
    }

    companion object {
        private const val KEYSTORE = "AndroidKeyStore"
        private const val ALIAS = "s5code.relay-dpop-key"

        /**
         * Loads the device's key, creating it on first use. One key for the
         * lifetime of the install: rotating it would orphan every relay link,
         * since the link records this thumbprint.
         */
        fun loadOrCreate(): DpopKey {
            val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
            val existing = keyStore.getEntry(ALIAS, null) as? KeyStore.PrivateKeyEntry
            if (existing != null) {
                return DpopKey(existing.privateKey, jwkOf(existing.certificate.publicKey as ECPublicKey))
            }
            val generator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, KEYSTORE)
            generator.initialize(
                KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256)
                    // Not user-authentication-bound: reconnects and notification
                    // taps have to mint proofs while the phone is locked.
                    .build()
            )
            val pair = generator.generateKeyPair()
            return DpopKey(pair.private, jwkOf(pair.public as ECPublicKey))
        }

        /** Drops the key, for sign-out. The next sign-in enrolls a fresh device. */
        fun clear() {
            runCatching {
                KeyStore.getInstance(KEYSTORE).apply { load(null) }.deleteEntry(ALIAS)
            }
        }

        internal fun jwkOf(publicKey: ECPublicKey): DpopPublicJwk =
            DpopPublicJwk(
                x = Base64Url.encode(coordinate(publicKey.w.affineX)),
                y = Base64Url.encode(coordinate(publicKey.w.affineY)),
            )

        /**
         * P-256 coordinates are fixed 32-byte big-endian values. `BigInteger`
         * gives a minimal encoding with an optional sign byte, so both padding and
         * trimming are needed; getting this wrong yields a key the relay reads as
         * a different device on roughly one key in 128.
         */
        internal fun coordinate(value: BigInteger): ByteArray {
            val raw = value.toByteArray()
            return when {
                raw.size == 32 -> raw
                raw.size > 32 -> raw.copyOfRange(raw.size - 32, raw.size)
                else -> ByteArray(32).also { raw.copyInto(it, 32 - raw.size) }
            }
        }

        /**
         * Converts the JCA's DER-encoded ECDSA signature into the fixed 64-byte
         * `R||S` form JOSE requires. The Keystore only speaks DER, and a relay
         * verifying a raw DER blob as ES256 rejects every proof.
         */
        internal fun derToJoseSignature(der: ByteArray): ByteArray {
            var offset = 0
            require(der.size > 8 && der[offset++] == 0x30.toByte()) { "Malformed ECDSA signature." }
            // Length byte: either short-form, or 0x81 followed by one length byte.
            if (der[offset].toInt() and 0xFF == 0x81) offset++
            offset++
            require(der[offset++] == 0x02.toByte()) { "Malformed ECDSA signature." }
            val rLength = der[offset++].toInt()
            val r = BigInteger(der.copyOfRange(offset, offset + rLength))
            offset += rLength
            require(der[offset++] == 0x02.toByte()) { "Malformed ECDSA signature." }
            val sLength = der[offset++].toInt()
            val s = BigInteger(der.copyOfRange(offset, offset + sLength))
            return coordinate(r) + coordinate(s)
        }
    }
}
