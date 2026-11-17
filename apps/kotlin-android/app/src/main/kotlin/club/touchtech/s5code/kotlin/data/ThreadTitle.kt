package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.feature.thread.collectComposerContextReferences
import club.touchtech.s5code.kotlin.model.ComposerAttachment

/** JavaScript's `\s`, which also matches the Unicode spaces Java's does not. */
private val TITLE_WHITESPACE = Regex("[\\s\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000\\uFEFF]+")

private fun normalizeTitleSeed(value: String): String =
    assistantCitationsToPlainText(value).trim().replace(TITLE_WHITESPACE, " ")

/** `truncate` in `packages/shared/src/String.ts`. */
private fun truncateTitle(text: String, maxLength: Int = 50): String {
    val trimmed = text.trim()
    return if (trimmed.length <= maxLength) trimmed else "${trimmed.take(maxLength)}..."
}

/**
 * `threadTitleTextFromPrompt` in client-runtime: context references read as
 * their labels, and image references drop out so a pasted image never titles a
 * thread with its markdown.
 */
internal fun threadTitleTextFromPrompt(value: String): String {
    val references = collectComposerContextReferences(value)
    val imageIds = mutableSetOf<String>()
    val prose = StringBuilder()
    var cursor = 0
    for (reference in references) {
        prose.append(value, cursor, reference.range.first)
        if (reference.kind == "image") imageIds += reference.contextId else prose.append(reference.label)
        cursor = reference.range.last + 1
    }
    prose.append(value, cursor, value.length)
    return normalizeTitleSeed(prose.toString()).ifEmpty {
        when (imageIds.size) {
            0 -> ""
            1 -> "Attached image"
            else -> "Attached images"
        }
    }
}

/** `deriveThreadTitleSeed` in client-runtime: the title a new thread starts with. */
internal fun deriveThreadTitleSeed(text: String, attachments: List<ComposerAttachment>): String {
    val prose = threadTitleTextFromPrompt(text)
    if (prose.isNotEmpty()) return truncateTitle(prose)
    val images = attachments.count { it.type == "image" }
    if (images > 0) return if (images == 1) "Attached image" else "Attached images"
    val attachmentName = normalizeTitleSeed(attachments.firstOrNull()?.name.orEmpty())
    if (attachmentName.isNotEmpty()) return truncateTitle("File: $attachmentName")
    return "New thread"
}
