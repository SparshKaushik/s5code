package club.touchtech.s5code.kotlin.connection

/**
 * Pairing URL parsing, matching `resolveRemotePairingTarget` in
 * `packages/shared/src/remote.ts` exactly. Getting this wrong is not a cosmetic
 * bug: the token is one-time, so a URL parsed loosely burns the credential and
 * then fails the exchange.
 *
 * Three shapes are accepted, in the order the shared helper checks them:
 *
 * 1. A direct pairing URL: `http://host:4488/pair#token=<credential>`. The token
 *    lives in the fragment so it never reaches a server log or a Referer header.
 * 2. A hosted pairing URL: `https://app.t3.codes/pair?host=<backend>#token=…`,
 *    where the backend to talk to is the `host` parameter, not the URL's own
 *    origin.
 * 3. A host plus a separately typed pairing code.
 */
data class PairingTarget(
    /** Base URL for HTTP calls, always with a trailing slash and no path. */
    val httpBaseUrl: String,
    /** Base URL for the WebSocket, same host with the ws/wss scheme. */
    val wsBaseUrl: String,
    val credential: String,
    /** Label offered by a hosted pairing link, when it carried one. */
    val label: String? = null,
) {
    val host: String
        get() = httpBaseUrl.removePrefix("http://").removePrefix("https://").trimEnd('/')

    val secure: Boolean
        get() = httpBaseUrl.startsWith("https://")
}

sealed interface PairingUrlResult {
    data class Valid(val target: PairingTarget) : PairingUrlResult

    data class Invalid(val reason: PairingUrlError) : PairingUrlResult
}

enum class PairingUrlError(val message: String) {
    Empty("Paste the pairing URL your server printed on startup."),
    NotAUrl("That doesn't look like a URL. It should start with http:// or https://."),
    UnsupportedScheme("Only http, https, ws, and wss pairing URLs are supported."),
    MissingHost("The URL is missing a host name."),
    MissingCode("The URL has no pairing token. Copy the whole line, including the part after #."),
    MissingHostForCode("Enter the address of the machine running the server."),
}

private val SUPPORTED_SCHEMES = setOf("http", "https", "ws", "wss")

/** Parses a pasted or scanned pairing URL. */
fun parsePairingUrl(input: String): PairingUrlResult {
    val trimmed = input.trim()
    if (trimmed.isEmpty()) return PairingUrlResult.Invalid(PairingUrlError.Empty)

    val url = ParsedUrl.parse(trimmed) ?: return PairingUrlResult.Invalid(PairingUrlError.NotAUrl)
    if (url.scheme !in SUPPORTED_SCHEMES) {
        return PairingUrlResult.Invalid(PairingUrlError.UnsupportedScheme)
    }
    if (url.authority.isEmpty() || url.authority.startsWith(":")) {
        return PairingUrlResult.Invalid(PairingUrlError.MissingHost)
    }

    val token = url.pairingToken()

    // Hosted pairing wins when both a host parameter and a token are present:
    // the credential belongs to the backend named by `host`, not to the web app
    // that served the link.
    val hostedBackend = url.queryValue("host")?.trim()
    if (!hostedBackend.isNullOrEmpty() && !token.isNullOrEmpty()) {
        val backend =
            ParsedUrl.parseWithDefaultScheme(hostedBackend)
                ?: return PairingUrlResult.Invalid(PairingUrlError.NotAUrl)
        if (backend.scheme !in SUPPORTED_SCHEMES) {
            return PairingUrlResult.Invalid(PairingUrlError.UnsupportedScheme)
        }
        return PairingUrlResult.Valid(
            PairingTarget(
                httpBaseUrl = backend.httpBaseUrl(),
                wsBaseUrl = backend.wsBaseUrl(),
                credential = token,
                label = url.queryValue("label")?.trim()?.takeIf { it.isNotEmpty() },
            )
        )
    }

    if (token.isNullOrEmpty()) return PairingUrlResult.Invalid(PairingUrlError.MissingCode)

    return PairingUrlResult.Valid(
        PairingTarget(
            httpBaseUrl = url.httpBaseUrl(),
            wsBaseUrl = url.wsBaseUrl(),
            credential = token,
        )
    )
}

/**
 * Builds a target from a separately entered host and pairing code, which is what
 * the manual form collects when someone reads a code off another screen.
 */
fun pairingTargetFor(host: String, code: String): PairingUrlResult {
    val trimmedHost = host.trim()
    if (trimmedHost.isEmpty()) return PairingUrlResult.Invalid(PairingUrlError.MissingHostForCode)
    val trimmedCode = code.trim()
    if (trimmedCode.isEmpty()) return PairingUrlResult.Invalid(PairingUrlError.MissingCode)

    val url =
        ParsedUrl.parseWithDefaultScheme(trimmedHost)
            ?: return PairingUrlResult.Invalid(PairingUrlError.NotAUrl)
    if (url.scheme !in SUPPORTED_SCHEMES) {
        return PairingUrlResult.Invalid(PairingUrlError.UnsupportedScheme)
    }
    if (url.authority.isEmpty()) return PairingUrlResult.Invalid(PairingUrlError.MissingHost)

    return PairingUrlResult.Valid(
        PairingTarget(
            httpBaseUrl = url.httpBaseUrl(),
            wsBaseUrl = url.wsBaseUrl(),
            credential = trimmedCode,
        )
    )
}

/**
 * Minimal URL split. `java.net.URI` is avoided deliberately: it rejects the
 * unencoded characters that turn up in hand-copied URLs, and pairing input is
 * hand-copied by definition.
 */
private data class ParsedUrl(
    val scheme: String,
    val authority: String,
    val query: String,
    val fragment: String,
) {
    fun queryValue(key: String): String? = paramValue(query, key)

    /** Fragment first, mirroring the shared helper: a hash token outranks a query one. */
    fun pairingToken(): String? =
        paramValue(fragment, "token")?.takeIf { it.isNotBlank() }
            ?: paramValue(query, "token")?.takeIf { it.isNotBlank() }

    fun httpBaseUrl(): String {
        val httpScheme =
            when (scheme) {
                "ws" -> "http"
                "wss" -> "https"
                else -> scheme
            }
        return "$httpScheme://$authority/"
    }

    fun wsBaseUrl(): String {
        val wsScheme =
            when (scheme) {
                "http" -> "ws"
                "https" -> "wss"
                else -> scheme
            }
        return "$wsScheme://$authority/"
    }

    companion object {
        fun parse(raw: String): ParsedUrl? {
            val separator = raw.indexOf("://")
            if (separator <= 0) return null
            val scheme = raw.take(separator).lowercase()
            val rest = raw.drop(separator + 3)
            val authority = rest.takeWhile { it != '/' && it != '?' && it != '#' }
            val afterAuthority = rest.drop(authority.length)
            val fragment = afterAuthority.substringAfter('#', missingDelimiterValue = "")
            val query =
                afterAuthority
                    .substringBefore('#')
                    .substringAfter('?', missingDelimiterValue = "")
            return ParsedUrl(scheme, authority, query, fragment)
        }

        /** Bare hosts default to https, matching `normalizeRemoteBaseUrl`. */
        fun parseWithDefaultScheme(raw: String): ParsedUrl? {
            val cleaned = raw.trimStart('/')
            return if (SCHEME_PREFIX.containsMatchIn(cleaned)) parse(cleaned)
            else parse("https://$cleaned")
        }

        private val SCHEME_PREFIX = Regex("^[a-zA-Z][a-zA-Z\\d+-]*://")

        private fun paramValue(source: String, key: String): String? =
            source
                .split('&')
                .asSequence()
                .mapNotNull { pair ->
                    val name = pair.substringBefore('=')
                    val value = pair.substringAfter('=', missingDelimiterValue = "")
                    if (name.equals(key, ignoreCase = true) && value.isNotEmpty()) {
                        decodeComponent(value)
                    } else {
                        null
                    }
                }
                .firstOrNull()

        private fun decodeComponent(value: String): String =
            runCatching { java.net.URLDecoder.decode(value, "UTF-8") }.getOrDefault(value)
    }
}
