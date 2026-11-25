package club.touchtech.s5code.kotlin.transport

import java.net.URI

/**
 * Whether a route reliably answers within a few seconds: loopback or LAN, the
 * kinds `connectionRouteKind` in `packages/client-runtime` ranks first. A tailnet
 * over DERP, a VPN, the relay, or a public host can spike past that on a live
 * socket. Classification follows `packages/shared/src/hostClassification.ts`.
 */
internal fun answersQuickly(httpBaseUrl: String): Boolean {
    val host =
        runCatching { URI(httpBaseUrl).host }.getOrNull()
            ?.lowercase()
            ?.removePrefix("[")
            ?.removeSuffix("]")
            ?.trimEnd('.')
            ?.takeIf { it.isNotEmpty() }
            ?: return false
    if (host == "localhost" || host == "::1" || host == "::") return true
    if (host.endsWith(".ts.net") || host.startsWith("fd7a:115c:a1e0:")) return false
    if (host.endsWith(".localhost") || host.endsWith(".local") || host == "home.arpa" || host.endsWith(".home.arpa")) {
        return true
    }
    if (!host.contains('.') && !host.contains(':')) return true
    (ipv4(host) ?: ipv4(host.removePrefix("::ffff:").takeIf { host.startsWith("::ffff:") }))?.let { parts ->
        val (a, b) = parts
        // 100.64.0.0/10 is shared address space: a VPN, not the LAN.
        if (a == 100 && b in 64..127) return false
        return a == 0 || a == 10 || a == 127 || (a == 172 && b in 16..31) || (a == 192 && b == 168) ||
            (a == 169 && b == 254) || (a == 198 && b in 18..19)
    }
    val first = host.substringBefore(':').takeIf { host.contains(':') }?.toIntOrNull(16) ?: return false
    return (first and 0xfe00) == 0xfc00 || (first and 0xffc0) == 0xfe80
}

private fun ipv4(host: String?): List<Int>? {
    val parts = host?.split('.')?.map { it.toIntOrNull() ?: return null } ?: return null
    return parts.takeIf { it.size == 4 && it.all { part -> part in 0..255 } }
}
