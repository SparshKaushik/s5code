package club.touchtech.s5code.kotlin.cloud

import club.touchtech.s5code.kotlin.BuildConfig

/**
 * The public S5 Connect configuration this build was compiled with, mirroring
 * `resolveCloudPublicConfig` in `apps/mobile/src/features/cloud/publicConfig.ts`.
 *
 * All three values must be present for Connect to work, and a fork that builds
 * without them gets a client with cloud features switched off rather than a
 * crash: [configured] is what every screen gates on.
 *
 * The relay URL is required to be an absolute HTTPS origin, matching
 * `normalizeSecureRelayUrl`. A relay reached over cleartext would carry account
 * tokens in the clear, so an http:// value is treated as unconfigured rather
 * than used.
 */
data class CloudPublicConfig(
    val publishableKey: String?,
    val jwtTemplate: String?,
    val relayUrl: String?,
) {
    val configured: Boolean
        get() = publishableKey != null && jwtTemplate != null && relayUrl != null

    companion object {
        fun fromBuildConfig(): CloudPublicConfig =
            CloudPublicConfig(
                publishableKey = BuildConfig.CLERK_PUBLISHABLE_KEY.trimOrNull(),
                jwtTemplate = BuildConfig.CLERK_JWT_TEMPLATE.trimOrNull(),
                relayUrl = BuildConfig.RELAY_URL.trimOrNull()?.let(::normalizeSecureRelayUrl),
            )

        private fun String.trimOrNull(): String? = trim().takeIf { it.isNotEmpty() }

        /** Absolute HTTPS origin with a trailing slash, or null. */
        internal fun normalizeSecureRelayUrl(raw: String): String? {
            val url = runCatching { java.net.URI(raw) }.getOrNull() ?: return null
            if (url.scheme?.lowercase() != "https") return null
            val host = url.host?.takeIf { it.isNotBlank() } ?: return null
            val port = if (url.port > 0) ":${url.port}" else ""
            return "https://$host$port/"
        }
    }
}
