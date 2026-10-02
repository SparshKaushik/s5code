package club.touchtech.s5code.kotlin.feature.files

/**
 * How the attachment viewer renders a file, ported from `filePreviewKind` in
 * `packages/shared/src/filePreview.ts`. Classification is identical for sent
 * attachments and question-answer files: mime wins, then the extension when the
 * mime is generic, then text/… and structured application types.
 */
enum class AttachmentPreviewKind {
    Image,
    Video,
    Audio,
    Pdf,
    Html,
    Markdown,
    Text,
    Unsupported,
}

fun attachmentPreviewKind(name: String, mimeType: String): AttachmentPreviewKind {
    val mime = mimeType.substringBefore(';').trim().lowercase()
    val lower = name.lowercase()
    val extension = lower.substringAfterLast('.', "")
    // "" and octet-stream/plain mean "nobody recorded a type": the extension is
    // the only evidence left. A definite mime already answered the question, so
    // a ".mp4" name on a PDF must not override it.
    val generic = mime.isEmpty() || mime == "application/octet-stream" || mime == "text/plain"
    when {
        mime == "application/pdf" -> return AttachmentPreviewKind.Pdf
        mime == "text/html" -> return AttachmentPreviewKind.Html
        mime == "text/markdown" || mime == "text/x-markdown" ->
            return AttachmentPreviewKind.Markdown
        mime.startsWith("image/") -> return AttachmentPreviewKind.Image
        mime.startsWith("video/") -> return AttachmentPreviewKind.Video
        mime.startsWith("audio/") -> return AttachmentPreviewKind.Audio
    }
    if (generic) {
        val basename = lower.substringAfterLast('/').substringAfterLast('\\')
        when {
            extension == "pdf" -> return AttachmentPreviewKind.Pdf
            extension == "htm" || extension == "html" -> return AttachmentPreviewKind.Html
            extension in MARKDOWN_EXTENSIONS -> return AttachmentPreviewKind.Markdown
            extension in IMAGE_EXTENSIONS -> return AttachmentPreviewKind.Image
            extension in VIDEO_EXTENSIONS -> return AttachmentPreviewKind.Video
            extension in AUDIO_EXTENSIONS -> return AttachmentPreviewKind.Audio
            extension in TEXT_EXTENSIONS ||
                TEXT_BASENAMES.any { base -> basename == base || basename.startsWith("$base.") } ->
                return AttachmentPreviewKind.Text
        }
    }
    // `application/(json|*+json|xml|*+xml|javascript|x-javascript|yaml|x-yaml|
    // toml|sql)` — structured text a provider may label without a text/ mime.
    if (
        mime.startsWith("text/") ||
            (mime.startsWith("application/") &&
                (mime.removePrefix("application/") in STRUCTURED_TEXT_TYPES ||
                    mime.endsWith("+json") ||
                    mime.endsWith("+xml")))
    ) {
        return AttachmentPreviewKind.Text
    }
    return AttachmentPreviewKind.Unsupported
}

/**
 * Decodes a bounded prefix of an attachment as UTF-8 text, mirroring
 * `decodeFilePreviewText`: NUL bytes mean binary (showing replacement
 * characters as a document is a lie), and a fatal decoder rejects malformed
 * UTF-8 rather than guessing.
 */
fun decodeAttachmentPreviewText(bytes: ByteArray, truncated: Boolean): AttachmentPreviewText {
    val bounded =
        if (bytes.size > ATTACHMENT_TEXT_PREVIEW_MAX_BYTES) {
            bytes.copyOf(ATTACHMENT_TEXT_PREVIEW_MAX_BYTES)
        } else {
            bytes
        }
    if (bounded.any { it == 0.toByte() }) {
        throw IllegalArgumentException(
            "This file contains binary data and cannot be shown as text."
        )
    }
    val decoder = Charsets.UTF_8.newDecoder()
    decoder.onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
    decoder.onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
    val text =
        try {
            decoder.decode(java.nio.ByteBuffer.wrap(bounded)).toString()
        } catch (error: java.nio.charset.CharacterCodingException) {
            throw IllegalArgumentException(
                "This file is not UTF-8 text. Open it in another app to view its contents.",
                error,
            )
        }
    return AttachmentPreviewText(
        text = text,
        truncated = truncated || bytes.size > bounded.size,
    )
}

data class AttachmentPreviewText(val text: String, val truncated: Boolean)

/** `FILE_TEXT_PREVIEW_MAX_BYTES` — previews never read past 1 MB. */
const val ATTACHMENT_TEXT_PREVIEW_MAX_BYTES = 1024 * 1024

/** "3.2 MB" / "48 KB" — `formatAttachmentSize` in client-runtime. Never "0 KB". */
fun attachmentSizeLabel(sizeBytes: Long): String =
    if (sizeBytes >= 1024 * 1024) {
        String.format(java.util.Locale.US, "%.1f MB", sizeBytes / (1024.0 * 1024.0))
    } else {
        "${maxOf(1, (sizeBytes + 1023) / 1024)} KB"
    }

// Extension tables mirror the maps in filePreview.ts/video.ts. They are
// extension-only sets here because classification never needs the mime value.
private val IMAGE_EXTENSIONS =
    setOf("avif", "gif", "ico", "jpeg", "jpg", "png", "svg", "webp")

private val VIDEO_EXTENSIONS = setOf("avi", "m4v", "mkv", "mov", "mp4", "ogv", "webm")

private val AUDIO_EXTENSIONS =
    setOf("mp3", "wav", "ogg", "oga", "flac", "aac", "m4a", "opus", "aiff")

private val MARKDOWN_EXTENSIONS = setOf("md", "markdown", "mdown", "mkd", "mdx")

private val TEXT_EXTENSIONS =
    setOf(
        "txt", "log", "json", "jsonc", "jsonl", "ndjson", "yaml", "yml", "toml",
        "ini", "conf", "config", "env", "csv", "tsv", "xml", "css", "scss",
        "sass", "less", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts",
        "py", "pyi", "rb", "go", "rs", "swift", "kt", "kts", "java", "c", "h",
        "cc", "cpp", "hpp", "cs", "php", "sh", "bash", "zsh", "fish", "sql",
        "graphql", "gql", "vue", "svelte", "r", "lua", "ex", "exs", "erl", "hs",
        "clj", "dart", "diff", "patch", "lock", "properties", "gradle",
    )

/** Extensionless names that are always text: Dockerfile, Makefile, LICENSE… */
private val TEXT_BASENAMES =
    setOf(
        "dockerfile", "makefile", "gemfile", "rakefile", "license", "readme",
        ".gitignore", ".gitattributes", ".editorconfig", ".env",
    )

private val STRUCTURED_TEXT_TYPES =
    setOf("json", "xml", "javascript", "x-javascript", "yaml", "x-yaml", "toml", "sql")
