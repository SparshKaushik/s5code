package club.touchtech.s5code.kotlin.feature.thread

/** The provider's summary heading, or the first sentence of the latest thought. */
internal fun liveThoughtLine(markdown: String): String {
    val heading = Regex("^\\s*\\*\\*([^*\\r\\n]+)\\*\\*[ \\t]*\\r?(?:\\n|$)")
        .find(markdown)?.groupValues?.get(1)
    val text = (heading ?: markdown)
        .replace(Regex("!?\\[([^]]*)]\\([^)]*\\)"), "$1")
        .replace(Regex("(?m)^[ \\t]*(?:#{1,6}|[-*+]|\\d+\\.)[ \\t]+"), "")
        .replace(Regex("`+|\\*\\*|~~"), "")
        .replace(Regex("(^|[^\\w*])[*_]([^*_\\n]+)[*_](?![\\w*])"), "$1$2")
        .replace(Regex("\\s+"), " ").trim()
    if (heading != null) return text
    val end = Regex("[.?!][\"'”’)]?(?=\\s)").find(text)
    return if (end == null) text else text.substring(0, end.range.last + 1)
}
