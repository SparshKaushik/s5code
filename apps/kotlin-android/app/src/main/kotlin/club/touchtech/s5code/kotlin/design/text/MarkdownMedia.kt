package club.touchtech.s5code.kotlin.design.text

/**
 * What a Markdown image (`![alt](source)`) renders as, after
 * `classifyMarkdownImageSource` in client-runtime and mobile's
 * `resolveMarkdownMediaPreview`.
 */
sealed interface MarkdownEmbed {
    /** A URL the device loads itself. */
    data class Image(val uri: String) : MarkdownEmbed

    /** A direct audio URL, played inline. */
    data class Audio(val uri: String, val name: String) : MarkdownEmbed

    /** A direct PDF, office, table or video URL, handed to another app. */
    data class Document(val uri: String, val name: String) : MarkdownEmbed

    /** A file in the thread's workspace, opened in the file viewer. [path] is workspace-relative. */
    data class WorkspaceFile(val path: String, val name: String) : MarkdownEmbed

    /**
     * Nothing the app can show: a host path outside the workspace (the app
     * signs only `workspace-file` assets), a network path, or a blocked scheme.
     */
    data object Unavailable : MarkdownEmbed
}

private val DIRECT_SOURCE = Regex("^(?:https?:|data:|blob:|//)", RegexOption.IGNORE_CASE)
private val URI_SCHEME = Regex("^[A-Za-z][A-Za-z0-9+.-]*:")
private val WINDOWS_DRIVE = Regex("^[A-Za-z]:[\\\\/]")

// `AUDIO_MIME_TYPE_BY_EXTENSION` in packages/shared/src/mediaPreview.ts.
private val AUDIO_EXTENSIONS = setOf("mp3", "wav", "ogg", "oga", "flac", "aac", "m4a", "opus", "aiff", "weba")

// Mobile's `DOCUMENT_MIME_TYPES`, plus video: the app has no inline video player.
private val DOCUMENT_EXTENSIONS =
    setOf("pdf", "csv", "tsv", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "mp4", "m4v", "mov", "webm", "mkv")

fun classifyMarkdownEmbed(source: String, workspaceRoot: String?): MarkdownEmbed {
    val value = source.trim().removeSurrounding("<", ">")
    if (value.isEmpty() || value.startsWith("#") || value.startsWith("?")) return MarkdownEmbed.Unavailable
    if (DIRECT_SOURCE.containsMatchIn(value)) return classifyDirect(value)

    val path =
        if (value.startsWith("file:", ignoreCase = true)) {
            // Reuses the link parser so `file://server/share` stays a UNC path.
            (resolveMarkdownLinkPresentation(value) as? MarkdownLinkPresentation.File)?.path
                ?: return MarkdownEmbed.Unavailable
        } else {
            decodeMarkdownPath(value.substringBefore('#').substringBefore('?')).removeWindowsFileSlash()
        }
    if (path.isBlank()) return MarkdownEmbed.Unavailable
    val absolute = path.startsWith('/') || path.startsWith('\\') || WINDOWS_DRIVE.containsMatchIn(path)
    // `~/` is the host's home, which rendering never guesses at.
    if (!absolute && (URI_SCHEME.containsMatchIn(path) || path.startsWith("~"))) return MarkdownEmbed.Unavailable
    // Rendering is not a click: a relative embed needs the workspace it is relative to.
    if (workspaceRoot.isNullOrBlank()) return MarkdownEmbed.Unavailable
    // A network path outside the project's own share falls out here too (a12d6ed942).
    val relative = resolveWorkspaceRelativeFilePath(workspaceRoot, path) ?: return MarkdownEmbed.Unavailable
    return MarkdownEmbed.WorkspaceFile(relative, relative.substringAfterLast('/'))
}

private fun classifyDirect(value: String): MarkdownEmbed {
    if (value.startsWith("data:", ignoreCase = true)) {
        return if (value.startsWith("data:image/", ignoreCase = true)) MarkdownEmbed.Image(value) else MarkdownEmbed.Unavailable
    }
    // A blob URL belongs to the page that minted it; there is no page here.
    if (value.startsWith("blob:", ignoreCase = true)) return MarkdownEmbed.Unavailable
    val uri = if (value.startsWith("//")) "https:$value" else value
    val name =
        decodeMarkdownPath(
            uri.substringBefore('#').substringBefore('?').substringAfter("://").substringAfter('/', "").substringAfterLast('/')
        )
    val extension = name.substringAfterLast('.', "").lowercase()
    return when (extension) {
        in AUDIO_EXTENSIONS -> MarkdownEmbed.Audio(uri, name)
        in DOCUMENT_EXTENSIONS -> MarkdownEmbed.Document(uri, name)
        // Extensionless and unknown URLs stay images, as an authored embed asks.
        else -> MarkdownEmbed.Image(uri)
    }
}
