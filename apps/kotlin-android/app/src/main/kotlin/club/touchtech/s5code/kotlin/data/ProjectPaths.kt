package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.FilesystemBrowseEntryDto

/**
 * Project-path and browse-path helpers.
 *
 * These are direct ports of `packages/shared/src/path.ts`,
 * `packages/client-runtime/src/state/projects.ts`, `state/filesystem.ts`, and
 * the add-project pieces of `operations/projects.ts`, so a path typed into this
 * client resolves to the same destination the RN client would send.
 */

fun isWindowsDrivePath(value: String): Boolean = Regex("^[a-zA-Z]:([/\\\\]|$)").containsMatchIn(value)

fun isUncPath(value: String): Boolean = value.startsWith("\\\\")

fun isWindowsAbsolutePath(value: String): Boolean = isUncPath(value) || isWindowsDrivePath(value)

fun isWindowsPlatform(platform: String): Boolean = Regex("^win(dows)?", RegexOption.IGNORE_CASE).containsMatchIn(platform)

fun isExplicitRelativePath(value: String): Boolean =
    value == "." || value == ".." ||
        value.startsWith("./") || value.startsWith("../") ||
        value.startsWith(".\\") || value.startsWith("..\\")

private fun isRootPath(value: String): Boolean =
    value == "/" || value == "\\" || Regex("^[a-zA-Z]:[/\\\\]$").matches(value)

private fun trimTrailingPathSeparators(value: String): String {
    if (value.isEmpty() || isRootPath(value)) return value
    val trimmed =
        if (value.startsWith("/")) value.replace(Regex("/+$"), "")
        else value.replace(Regex("[\\\\/]+$"), "")
    if (trimmed.isEmpty()) return value
    return if (Regex("^[a-zA-Z]:$").matches(trimmed)) "$trimmed\\" else trimmed
}

fun normalizeProjectPathForDispatch(value: String): String = trimTrailingPathSeparators(value.trim())

fun normalizeProjectPathForComparison(value: String): String {
    val normalized = normalizeProjectPathForDispatch(value)
    if (isWindowsDrivePath(normalized) || isUncPath(normalized)) {
        return normalized.replace('/', '\\').lowercase()
    }
    return normalized
}

private fun absolutePathKind(value: String): String? =
    when {
        isWindowsDrivePath(value) || isUncPath(value) -> "windows"
        value.startsWith("/") -> "unix"
        else -> null
    }

private fun preferredPathSeparator(value: String): Char =
    when (absolutePathKind(value)) {
        "windows" -> '\\'
        "unix" -> '/'
        else -> if (value.contains('\\')) '\\' else '/'
    }

private fun hasTrailingPathSeparator(value: String): Boolean =
    if (absolutePathKind(value) == "unix") value.endsWith("/")
    else Regex("[\\\\/]$").containsMatchIn(value)

private fun splitPathSegments(value: String, separator: Char): List<String> =
    value.split(if (separator == '/') Regex("/+") else Regex("[\\\\/]+")).filter { it.isNotEmpty() }

private fun lastPathSeparatorIndex(value: String): Int =
    if (absolutePathKind(value) == "unix") value.lastIndexOf('/')
    else maxOf(value.lastIndexOf('/'), value.lastIndexOf('\\'))

private data class AbsolutePath(val root: String, val separator: Char, val segments: List<String>)

private fun splitAbsolutePath(value: String): AbsolutePath? =
    when {
        isWindowsDrivePath(value) -> {
            val root = "${value.substring(0, 2)}\\"
            AbsolutePath(root, '\\', splitPathSegments(value.substring(root.length), '\\'))
        }
        isUncPath(value) -> {
            val segments = splitPathSegments(value, '\\')
            if (segments.size < 2) return null
            val root = "\\\\${segments[0]}\\${segments[1]}\\"
            AbsolutePath(root, '\\', segments.drop(2))
        }
        value.startsWith("/") ->
            AbsolutePath("/", '/', splitPathSegments(value.substring(1), '/'))
        else -> null
    }

/** Whether [value] reads as a browseable filesystem path on [platform]. */
fun isFilesystemBrowseQuery(value: String, platform: String = ""): Boolean {
    val allowWindowsPaths = isWindowsPlatform(platform)
    return value.startsWith("./") || value.startsWith("../") ||
        value.startsWith(".\\") || value.startsWith("..\\") ||
        value.startsWith("/") || value.startsWith("~/") ||
        (allowWindowsPaths && isWindowsAbsolutePath(value))
}

fun isUnsupportedWindowsProjectPath(value: String, platform: String): Boolean =
    isWindowsAbsolutePath(value) && !isWindowsPlatform(platform)

fun resolveProjectPathForDispatch(value: String, cwd: String? = null): String {
    val trimmed = value.trim()
    if (!isExplicitRelativePath(trimmed) || cwd == null) {
        return normalizeProjectPathForDispatch(trimmed)
    }
    val absoluteBase = splitAbsolutePath(normalizeProjectPathForDispatch(cwd))
        ?: return normalizeProjectPathForDispatch(trimmed)
    val nextSegments = absoluteBase.segments.toMutableList()
    for (segment in trimmed.split(Regex("[\\\\/]+"))) {
        if (segment.isEmpty() || segment == ".") continue
        if (segment == "..") {
            if (nextSegments.isNotEmpty()) nextSegments.removeAt(nextSegments.lastIndex)
            continue
        }
        nextSegments += segment
    }
    val joined = nextSegments.joinToString(absoluteBase.separator.toString())
    return normalizeProjectPathForDispatch(
        if (joined.isEmpty()) absoluteBase.root else "${absoluteBase.root}$joined"
    )
}

/**
 * `resolveAddProjectPath`: the path validation the add-project screens apply
 * before dispatching. No active project exists in this flow, so an explicit
 * relative path is rejected rather than resolved.
 */
fun resolveAddProjectPath(rawPath: String, platform: String): Pair<String?, String?> {
    val raw = rawPath.trim()
    if (raw.isEmpty()) return null to "Enter a project path."
    if (isUnsupportedWindowsProjectPath(raw, platform)) {
        return null to "Windows-style paths are only supported on Windows environments."
    }
    if (isExplicitRelativePath(raw)) {
        return null to "Relative paths require an active project in this environment."
    }
    val path = resolveProjectPathForDispatch(raw, null)
    return if (path.isEmpty()) null to "Enter a project path." else path to null
}

/** Last path segment, the name a server would infer for the project. */
fun inferProjectTitleFromPath(value: String): String {
    val normalized = normalizeProjectPathForDispatch(value)
    val absolute = splitAbsolutePath(normalized)
    if (absolute != null) return absolute.segments.lastOrNull() ?: normalized
    return normalized.split(Regex("[/\\\\]")).lastOrNull { it.isNotEmpty() } ?: normalized
}

/* ── Browse-path helpers (state/filesystem.ts) ───────────────────────── */

fun appendBrowsePathSegment(currentPath: String, segment: String): String {
    val separator = preferredPathSeparator(currentPath)
    return "${getBrowseDirectoryPath(currentPath)}$segment$separator"
}

fun getBrowseLeafPathSegment(currentPath: String): String {
    val last = lastPathSeparatorIndex(currentPath)
    return currentPath.substring(last + 1)
}

fun getBrowseDirectoryPath(currentPath: String): String {
    if (hasTrailingPathSeparator(currentPath)) return currentPath
    val last = lastPathSeparatorIndex(currentPath)
    return if (last < 0) currentPath else currentPath.substring(0, last + 1)
}

fun ensureBrowseDirectoryPath(currentPath: String): String {
    val trimmed = currentPath.trim()
    if (trimmed.isEmpty() || hasTrailingPathSeparator(trimmed)) return trimmed
    return "$trimmed${preferredPathSeparator(trimmed)}"
}

fun getBrowseParentPath(currentPath: String): String? {
    val trimmed = normalizeProjectPathForDispatch(currentPath)
    val absolute = splitAbsolutePath(trimmed)
    if (absolute != null) {
        if (absolute.segments.isEmpty()) return null
        if (absolute.segments.size == 1) return absolute.root
        val parents = absolute.segments.dropLast(1).joinToString(absolute.separator.toString())
        return "${absolute.root}$parents${absolute.separator}"
    }
    val separator = preferredPathSeparator(trimmed)
    val last = lastPathSeparatorIndex(trimmed)
    if (last < 0) return null
    if (last == 2 && Regex("^[a-zA-Z]:").containsMatchIn(trimmed)) {
        return "${trimmed.substring(0, 2)}$separator"
    }
    return trimmed.substring(0, last + 1)
}

fun canNavigateUp(currentPath: String): Boolean =
    hasTrailingPathSeparator(currentPath) && getBrowseParentPath(currentPath) != null

/** `getFilesystemBrowsePath`: what a typed path means for the folder list. */
data class FilesystemBrowsePath(
    val isBrowsing: Boolean,
    val directoryPath: String,
    val filterQuery: String,
    val parentPath: String?,
    val canBrowseUp: Boolean,
)

fun getFilesystemBrowsePath(query: String, platform: String = ""): FilesystemBrowsePath {
    val isBrowsing = isFilesystemBrowseQuery(query, platform)
    val directoryPath = if (isBrowsing) getBrowseDirectoryPath(query) else ""
    val filterQuery =
        if (isBrowsing && !hasTrailingPathSeparator(query)) getBrowseLeafPathSegment(query) else ""
    val parentPath = if (isBrowsing) getBrowseParentPath(directoryPath) else null
    return FilesystemBrowsePath(
        isBrowsing = isBrowsing,
        directoryPath = directoryPath,
        filterQuery = filterQuery,
        parentPath = parentPath,
        canBrowseUp = isBrowsing && parentPath != null && hasTrailingPathSeparator(directoryPath),
    )
}

/** `filterFilesystemBrowseEntries`: prefix filter, hidden entries need a leading dot. */
fun filterFilesystemBrowseEntries(
    entries: List<FilesystemBrowseEntryDto>,
    query: String,
): List<FilesystemBrowseEntryDto> {
    val lower = query.lowercase()
    val showHidden = query.startsWith(".")
    return entries.filter { entry ->
        entry.name.lowercase().startsWith(lower) && (showHidden || !entry.name.startsWith("."))
    }
}

/* ── Clone-destination helpers (operations/projects.ts) ──────────────── */

/**
 * The folder name `git clone` would pick, from either a looked-up `owner/repo`
 * or a pasted URL in any form. Always the last segment minus `.git`.
 */
fun getCloneDirectoryName(repositoryOrRemoteUrl: String?): String {
    val withoutQuery = (repositoryOrRemoteUrl ?: "").split(Regex("[?#]"))[0].trim()
    val schemeIndex = withoutQuery.indexOf("://")
    val hasHost = schemeIndex >= 0 || Regex("^[^/\\\\:]+@[^/\\\\:]+:").containsMatchIn(withoutQuery)
    val pathPart = if (schemeIndex >= 0) withoutQuery.substring(schemeIndex + 3) else withoutQuery
    val segments = pathPart.split(Regex("[/\\\\:]+")).filter { it.trim().isNotEmpty() }
    if (hasHost && segments.size < 2) return ""
    val last = segments.lastOrNull()?.trim() ?: ""
    // A port can only sit directly behind the authority, so it is a port only
    // when nothing follows it.
    if (hasHost && segments.size == 2 && Regex("^\\d+$").matches(last)) return ""
    return if (last.endsWith(".git")) last.removeSuffix(".git") else last
}

/** Clone destination for a directory: the directory plus the repo folder inside. */
fun getCloneDestinationPath(directoryPath: String, directoryName: String?): String {
    val name = directoryName?.trim() ?: ""
    if (name.isEmpty()) return directoryPath
    return "${ensureBrowseDirectoryPath(directoryPath)}$name"
}

/**
 * `getCloneDestinationBrowsePath`: after choosing a directory while the clone
 * folder is pinned in the input. Picking an existing directory that already
 * matches the pinned name uses it directly instead of producing `repo/repo`.
 */
fun getCloneDestinationBrowsePath(
    browseDirectoryPath: String,
    selectedDirectoryName: String,
    cloneDirectoryName: String,
    caseSensitive: Boolean,
): String {
    val selectedDirectoryPath = appendBrowsePathSegment(browseDirectoryPath, selectedDirectoryName)
    val matches =
        if (caseSensitive) selectedDirectoryName == cloneDirectoryName
        else selectedDirectoryName.equals(cloneDirectoryName, ignoreCase = true)
    return if (matches) selectedDirectoryPath
    else getCloneDestinationPath(selectedDirectoryPath, cloneDirectoryName)
}

private val GITHUB_REPOSITORY_SHORTHAND =
    Regex("^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})/[A-Za-z0-9._-]+(?:\\.git)?$")

/** `normalizePastedCloneUrl`: `owner/repo` shorthand becomes a GitHub HTTPS URL. */
fun normalizePastedCloneUrl(input: String): String {
    val trimmed = input.trim()
    if (!GITHUB_REPOSITORY_SHORTHAND.matches(trimmed)) return trimmed
    val repository = if (trimmed.endsWith(".git")) trimmed else "$trimmed.git"
    return "https://github.com/$repository"
}

/** GitHub and Forgejo default to HTTPS; other providers keep their SSH default. */
fun defaultCloneUrl(provider: String, url: String, sshUrl: String): String =
    if (provider == "github" || provider == "forgejo") url else sshUrl
