package club.touchtech.s5code.kotlin.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.core.content.edit
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** A paired environment and the token that reaches it. */
@Serializable
data class SavedEnvironment(
    val environmentId: String,
    val label: String,
    val httpBaseUrl: String,
    val wsBaseUrl: String,
    /**
     * Encrypted at rest with a Keystore key. Held in memory as ciphertext and
     * decrypted per use so a heap dump does not hand over every environment.
     */
    val encryptedToken: String,
    val platform: String = "",
    val serverVersion: String = "",
    val pairedAtMillis: Long = 0,
    /**
     * How this environment was reached, and therefore how its token is used.
     * Absent in rows written by earlier builds, which were all direct.
     */
    val kind: SavedEnvironmentKind = SavedEnvironmentKind.Direct,
) {
    /**
     * True when the saved token is DPoP-bound and has to be re-minted through the
     * relay rather than reused. Relay-issued environment tokens are short-lived by
     * design, so a cloud row is always re-authorized on connect.
     */
    val relayManaged: Boolean
        get() = kind == SavedEnvironmentKind.Cloud
}

enum class SavedEnvironmentKind {
    Direct,
    Cloud,
}

/**
 * Persistent list of paired environments plus their access tokens.
 *
 * Tokens are encrypted with an AES-GCM key held in the Android Keystore, so the
 * key material never enters the process: the app can ask the Keystore to decrypt
 * but cannot read or copy the key. That matters because these tokens are
 * long-lived and grant full orchestration authority over a developer's machine.
 *
 * `SharedPreferences` carries the ciphertext rather than DataStore, deliberately:
 * this store is read once during the cold-start gate and written only on
 * pair/unpair, so a flow-based store would add a dependency for a file that is
 * touched twice per launch. `apply()` is never used — a token written
 * asynchronously and lost to a crash means a spent pairing code and a re-pair.
 */
class EnvironmentStore(context: Context) {

    private val preferences =
        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    private val json = Json { ignoreUnknownKeys = true }

    private val _environments = MutableStateFlow<List<SavedEnvironment>>(emptyList())
    val environments: StateFlow<List<SavedEnvironment>> = _environments.asStateFlow()

    /** True once [load] has run, so the UI can gate on a real answer. */
    private val _loaded = MutableStateFlow(false)
    val loaded: StateFlow<Boolean> = _loaded.asStateFlow()

    suspend fun load() {
        val saved =
            withContext(Dispatchers.IO) {
                val raw = preferences.getString(KEY_ENVIRONMENTS, null) ?: return@withContext emptyList()
                runCatching { json.decodeFromString<List<SavedEnvironment>>(raw) }
                    // A store this build cannot read is treated as empty rather
                    // than fatal: re-pairing is a minor annoyance, a launch loop
                    // is not.
                    .getOrDefault(emptyList())
            }
        _environments.value = saved
        _loaded.value = true
    }

    /**
     * Saves an environment, replacing any entry with the same id. Re-pairing an
     * environment should update it in place rather than producing two rows for
     * one machine.
     */
    suspend fun save(
        environmentId: String,
        label: String,
        httpBaseUrl: String,
        wsBaseUrl: String,
        token: String,
        platform: String,
        serverVersion: String,
        kind: SavedEnvironmentKind = SavedEnvironmentKind.Direct,
    ) {
        val entry =
            SavedEnvironment(
                environmentId = environmentId,
                label = label,
                httpBaseUrl = httpBaseUrl,
                wsBaseUrl = wsBaseUrl,
                encryptedToken = encrypt(token),
                platform = platform,
                serverVersion = serverVersion,
                pairedAtMillis = System.currentTimeMillis(),
                kind = kind,
            )
        persist(_environments.value.filterNot { it.environmentId == environmentId } + entry)
    }

    suspend fun rename(environmentId: String, label: String) {
        persist(
            _environments.value.map {
                if (it.environmentId == environmentId) it.copy(label = label) else it
            }
        )
    }

    /**
     * Records the endpoint an environment is currently reachable at. Relay-managed
     * tunnels get a new hostname when they are re-provisioned, and the saved row
     * has to follow or the next cold start dials a host that no longer resolves.
     * A no-op write is skipped so a reconnect loop does not rewrite the file.
     */
    suspend fun updateEndpoint(environmentId: String, httpBaseUrl: String, wsBaseUrl: String) {
        val current = _environments.value.firstOrNull { it.environmentId == environmentId } ?: return
        if (current.httpBaseUrl == httpBaseUrl && current.wsBaseUrl == wsBaseUrl) return
        persist(
            _environments.value.map {
                if (it.environmentId == environmentId) {
                    it.copy(httpBaseUrl = httpBaseUrl, wsBaseUrl = wsBaseUrl)
                } else {
                    it
                }
            }
        )
    }

    /**
     * Saves a relay-managed environment with no usable token of its own: the
     * credential is minted per connection, so the stored row is the link, not the
     * secret.
     */
    suspend fun saveManaged(
        environmentId: String,
        label: String,
        httpBaseUrl: String,
        wsBaseUrl: String,
        platform: String,
        serverVersion: String,
    ) = save(
        environmentId = environmentId,
        label = label,
        httpBaseUrl = httpBaseUrl,
        wsBaseUrl = wsBaseUrl,
        // Deliberately empty: nothing reads a cloud row's token, and writing a
        // real one would leave a bearer credential on disk that outlives its use.
        token = "",
        platform = platform,
        serverVersion = serverVersion,
        kind = SavedEnvironmentKind.Cloud,
    )

    suspend fun remove(environmentId: String) {
        persist(_environments.value.filterNot { it.environmentId == environmentId })
    }

    suspend fun clear() = persist(emptyList())

    /** Decrypts one token for immediate use. Null when the key is gone. */
    fun token(environment: SavedEnvironment): String? = decrypt(environment.encryptedToken)

    private suspend fun persist(next: List<SavedEnvironment>) {
        _environments.value = next
        withContext(Dispatchers.IO) {
            // Committed rather than applied: the token has to be on disk before the
            // pairing call returns, since the credential that produced it is spent.
            preferences.edit(commit = true) { putString(KEY_ENVIRONMENTS, json.encodeToString(next)) }
        }
    }

    /* ── Keystore ────────────────────────────────────────────────────── */

    private fun secretKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                // Deliberately not user-authentication-bound: notifications and
                // background reconnects have to work while the phone is locked,
                // and a token that only decrypts after a biometric prompt would
                // break every one of them.
                .setRandomizedEncryptionRequired(true)
                .build()
        )
        return generator.generateKey()
    }

    private fun encrypt(value: String): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, secretKey())
        val encrypted = cipher.doFinal(value.toByteArray())
        // The IV is generated per encryption and stored beside the ciphertext.
        // Reusing one across tokens would be a real break of GCM, so it is never
        // derived or hard-coded.
        return Base64.encodeToString(cipher.iv, Base64.NO_WRAP) +
            IV_SEPARATOR +
            Base64.encodeToString(encrypted, Base64.NO_WRAP)
    }

    private fun decrypt(value: String): String? {
        val parts = value.split(IV_SEPARATOR)
        if (parts.size != 2) return null
        return runCatching {
                val cipher = Cipher.getInstance(TRANSFORMATION)
                cipher.init(
                    Cipher.DECRYPT_MODE,
                    secretKey(),
                    GCMParameterSpec(GCM_TAG_BITS, Base64.decode(parts[0], Base64.NO_WRAP)),
                )
                String(cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP)))
            }
            .getOrNull()
    }

    private companion object {
        const val PREFERENCES_NAME = "s5code.environments"
        const val KEY_ENVIRONMENTS = "environments"
        const val KEYSTORE_PROVIDER = "AndroidKeyStore"
        const val KEY_ALIAS = "s5code.environment-tokens"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val GCM_TAG_BITS = 128
        const val IV_SEPARATOR = ":"
    }
}
