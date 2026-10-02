package club.touchtech.s5code.kotlin.design.text

private val WINDOWS_DRIVE_PATH = Regex("^[A-Za-z]:[\\\\/]")
private val RELATIVE_PREFIX = Regex("^(~/|\\.{1,2}/)")
private val RELATIVE_FILE_PATH = Regex("^[A-Za-z0-9._-]+(?:/[A-Za-z0-9._-]+)+(?::\\d+){0,2}$")
private val RELATIVE_FILE_NAME = Regex("^[A-Za-z0-9._-]+\\.[A-Za-z0-9_-]+(?::\\d+){0,2}$")
private val POSITION_SUFFIX = Regex(":(\\d+)(?::(\\d+))?$")
private val POSIX_ROOT_PREFIXES =
    listOf("/Users/", "/home/", "/tmp/", "/var/", "/etc/", "/opt/", "/mnt/", "/Volumes/", "/private/", "/root/")

enum class MarkdownFileIcon {
    Agents, Astro, Babel, Bash, Biome, Bootstrap, Browserslist, Bun, C, Claude, Cpp, Css, Database,
    Default, Docker, Eslint, Font, Git, Go, Graphql, Html, Image, Javascript, Json, Markdown, Mcp,
    Nextjs, Npm, Oxc, Package, Pnpm, Postcss, Prettier, Python, React, Readme, Ruby, Rust, Sass,
    Stylelint, Svelte, Svg, Svgo, Swift, Table, Tailwind, Terraform, Text, Tsconfig, Typescript, Vite,
    Vscode, Vue, Wasm, Webpack, Yml, Zig, Zip,
}

sealed interface MarkdownLinkPresentation {
    data class External(val href: String, val host: String) : MarkdownLinkPresentation

    data class File(
        val href: String,
        val icon: MarkdownFileIcon,
        val label: String,
        val path: String,
        val line: Int? = null,
        val column: Int? = null,
    ) : MarkdownLinkPresentation

    data class Link(val href: String?) : MarkdownLinkPresentation
}

private val ICON_BY_NAME =
    mapOf(
        ".babelrc" to MarkdownFileIcon.Babel,
        ".bash_profile" to MarkdownFileIcon.Bash,
        ".bashrc" to MarkdownFileIcon.Bash,
        ".browserslistrc" to MarkdownFileIcon.Browserslist,
        ".dockerignore" to MarkdownFileIcon.Docker,
        ".gitattributes" to MarkdownFileIcon.Git,
        ".gitignore" to MarkdownFileIcon.Git,
        ".gitkeep" to MarkdownFileIcon.Git,
        ".gitmodules" to MarkdownFileIcon.Git,
        ".terraform.lock.hcl" to MarkdownFileIcon.Terraform,
        ".zprofile" to MarkdownFileIcon.Bash,
        ".zshenv" to MarkdownFileIcon.Bash,
        ".zshrc" to MarkdownFileIcon.Bash,
        "agents.md" to MarkdownFileIcon.Agents,
        "biome.json" to MarkdownFileIcon.Biome,
        "bun.lock" to MarkdownFileIcon.Bun,
        "bun.lockb" to MarkdownFileIcon.Bun,
        "bunfig.toml" to MarkdownFileIcon.Bun,
        "claude.md" to MarkdownFileIcon.Claude,
        "compose.yaml" to MarkdownFileIcon.Docker,
        "compose.yml" to MarkdownFileIcon.Docker,
        "docker-compose.yaml" to MarkdownFileIcon.Docker,
        "docker-compose.yml" to MarkdownFileIcon.Docker,
        "dockerfile" to MarkdownFileIcon.Docker,
        "gemfile" to MarkdownFileIcon.Ruby,
        "package.json" to MarkdownFileIcon.Package,
        "pnpm-lock.yaml" to MarkdownFileIcon.Pnpm,
        "pnpm-workspace.yaml" to MarkdownFileIcon.Pnpm,
        "rakefile" to MarkdownFileIcon.Ruby,
        "readme.md" to MarkdownFileIcon.Readme,
        "tsconfig.json" to MarkdownFileIcon.Tsconfig,
    )

private val CONFIG_PREFIXES =
    listOf(
        "babel" to MarkdownFileIcon.Babel,
        "eslint" to MarkdownFileIcon.Eslint,
        "next.config" to MarkdownFileIcon.Nextjs,
        "oxlint" to MarkdownFileIcon.Oxc,
        "postcss" to MarkdownFileIcon.Postcss,
        "prettier" to MarkdownFileIcon.Prettier,
        "stylelint" to MarkdownFileIcon.Stylelint,
        "svgo" to MarkdownFileIcon.Svgo,
        "tailwind" to MarkdownFileIcon.Tailwind,
        "vite.config" to MarkdownFileIcon.Vite,
        "webpack.config" to MarkdownFileIcon.Webpack,
    )

private val ICON_BY_EXTENSION =
    buildMap {
        fun icon(value: MarkdownFileIcon, vararg extensions: String) = extensions.forEach { put(it, value) }
        icon(MarkdownFileIcon.Zip, "7z", "bz2", "gz", "jar", "rar", "tar", "tgz", "zip")
        icon(MarkdownFileIcon.Astro, "astro")
        icon(MarkdownFileIcon.Vscode, "code-workspace")
        icon(MarkdownFileIcon.Bash, "bash", "fish", "sh", "zsh")
        icon(MarkdownFileIcon.Image, "avif", "bmp", "gif", "ico", "icns", "jpeg", "jpg", "png", "webp")
        icon(MarkdownFileIcon.C, "c", "h")
        icon(MarkdownFileIcon.Cpp, "cc", "cpp", "cxx", "hh", "hpp", "hxx", "inl")
        icon(MarkdownFileIcon.Css, "css", "less", "postcss")
        icon(MarkdownFileIcon.Table, "csv", "tsv")
        icon(MarkdownFileIcon.Database, "db", "sql", "sqlite", "sqlite3")
        icon(MarkdownFileIcon.Text, "env", "env.development", "env.local", "env.production", "ini", "txt")
        icon(MarkdownFileIcon.Font, "eot", "woff", "woff2")
        icon(MarkdownFileIcon.Go, "go")
        icon(MarkdownFileIcon.Graphql, "gql", "graphql")
        icon(MarkdownFileIcon.Html, "htm", "html")
        icon(MarkdownFileIcon.Javascript, "js", "mjs")
        icon(MarkdownFileIcon.React, "jsx", "tsx")
        icon(MarkdownFileIcon.Json, "json", "jsonc")
        icon(MarkdownFileIcon.Markdown, "md", "mdx", "mdx.tsx")
        icon(MarkdownFileIcon.Typescript, "cts", "mts", "ts")
        icon(MarkdownFileIcon.Python, "py", "pyi", "pyw", "pyx")
        icon(MarkdownFileIcon.Ruby, "erb", "rake", "rb")
        icon(MarkdownFileIcon.Rust, "rs")
        icon(MarkdownFileIcon.Sass, "sass", "scss")
        icon(MarkdownFileIcon.Svelte, "svelte")
        icon(MarkdownFileIcon.Svg, "svg")
        icon(MarkdownFileIcon.Swift, "swift")
        icon(MarkdownFileIcon.Terraform, "tf", "tfstate", "tfvars")
        icon(MarkdownFileIcon.Vue, "vue")
        icon(MarkdownFileIcon.Wasm, "wasm")
        icon(MarkdownFileIcon.Yml, "yaml", "yml")
        icon(MarkdownFileIcon.Zig, "zig")
    }

fun resolveMarkdownFileIcon(value: String): MarkdownFileIcon {
    val basename = fileLabel(value).replace(POSITION_SUFFIX, "").lowercase()
    ICON_BY_NAME[basename]?.let { return it }
    if (basename.startsWith("tsconfig.") && basename.endsWith(".json")) return MarkdownFileIcon.Tsconfig
    CONFIG_PREFIXES.firstOrNull { basename.startsWith(it.first) }?.let { return it.second }
    val segments = basename.split('.')
    for (index in 1 until segments.size) {
        ICON_BY_EXTENSION[segments.drop(index).joinToString(".")]?.let { return it }
    }
    return MarkdownFileIcon.Default
}

fun resolveMarkdownLinkPresentation(href: String): MarkdownLinkPresentation {
    val normalized = href.trim().removeSurrounding("<", ">")
    runCatching { java.net.URI(normalized) }.getOrNull()?.let { uri ->
        if (uri.scheme.equals("http", true) || uri.scheme.equals("https", true)) {
            return MarkdownLinkPresentation.External(normalized, uri.host.orEmpty())
        }
    }

    val (sourcePath, hash) =
        if (normalized.startsWith("file:", ignoreCase = true)) {
            val uri = runCatching { java.net.URI(normalized) }.getOrNull()
            decode(uri?.path.orEmpty().removeWindowsFileSlash()) to uri?.fragment?.let { "#$it" }.orEmpty()
        } else {
            val hashIndex = normalized.indexOf('#')
            val pathWithQuery = if (hashIndex >= 0) normalized.take(hashIndex) else normalized
            decode(pathWithQuery.substringBefore('?')) to decode(if (hashIndex >= 0) normalized.drop(hashIndex) else "")
        }
    val target = splitPosition(sourcePath.trim(), hash.trim())
    if (target != null && looksLikeFilePath(target.pathWithPosition)) {
        return MarkdownLinkPresentation.File(
            href = normalized,
            icon = resolveMarkdownFileIcon(target.path),
            label = fileLabel(target.pathWithPosition),
            path = target.path,
            line = target.line,
            column = target.column,
        )
    }
    return MarkdownLinkPresentation.Link(normalized.takeIf { it.startsWith("mailto:", true) || it.startsWith("tel:", true) })
}

/** Constrains Markdown file links to this thread's project/worktree. */
fun resolveWorkspaceRelativeFilePath(workspaceRoot: String?, targetPath: String): String? {
    if (!isAbsolutePath(targetPath)) {
        if (targetPath.startsWith("~/") || targetPath.startsWith("~\\")) return null
        return normalizeRelativePath(targetPath)
    }
    if (workspaceRoot.isNullOrBlank()) return null
    val target = targetPath.replace('\\', '/')
    val root = workspaceRoot.replace('\\', '/').trimEnd('/')
    val insensitive = WINDOWS_DRIVE_PATH.containsMatchIn(targetPath) || WINDOWS_DRIVE_PATH.containsMatchIn(workspaceRoot)
    val comparableTarget = if (insensitive) target.lowercase() else target
    val comparableRoot = if (insensitive) root.lowercase() else root
    if (!comparableTarget.startsWith("$comparableRoot/")) return null
    return normalizeRelativePath(target.drop(root.length + 1))
}

private data class PositionedPath(
    val path: String,
    val line: Int?,
    val column: Int?,
) {
    val pathWithPosition = path + (line?.let { ":$it${column?.let { value -> ":$value" }.orEmpty()}" }.orEmpty())
}

private fun splitPosition(path: String, hash: String): PositionedPath? {
    if (path.isBlank()) return null
    val suffix = POSITION_SUFFIX.find(path)
    val hashMatch = if (suffix == null) Regex("^#L(\\d+)(?:C(\\d+))?$", RegexOption.IGNORE_CASE).find(hash) else null
    val match = suffix ?: hashMatch
    val line = match?.groupValues?.getOrNull(1)?.toIntOrNull()?.takeIf { it > 0 }
    val column = match?.groupValues?.getOrNull(2)?.toIntOrNull()?.takeIf { it > 0 }
    val cleanPath = if (suffix != null) path.removeRange(suffix.range) else path
    return PositionedPath(cleanPath, line, column)
}

private fun normalizeRelativePath(value: String): String? {
    val segments = mutableListOf<String>()
    value.replace('\\', '/').split('/').forEach { segment ->
        when {
            segment.isEmpty() || segment == "." -> Unit
            segment == ".." && segments.isEmpty() -> return null
            segment == ".." -> segments.removeAt(segments.lastIndex)
            else -> segments += segment
        }
    }
    return segments.joinToString("/").takeIf(String::isNotEmpty)
}

private fun looksLikeFilePath(value: String): Boolean {
    if (WINDOWS_DRIVE_PATH.containsMatchIn(value) || value.startsWith("\\\\")) return true
    if (RELATIVE_PREFIX.containsMatchIn(value)) return true
    if (value.startsWith('/')) {
        if (POSIX_ROOT_PREFIXES.any(value::startsWith)) return true
        val basename = value.substringAfterLast('/')
        return '.' in basename || POSITION_SUFFIX.containsMatchIn(value)
    }
    val basename = value.replace(POSITION_SUFFIX, "").lowercase()
    return basename in ICON_BY_NAME || RELATIVE_FILE_PATH.matches(value) || RELATIVE_FILE_NAME.matches(value)
}

private fun isAbsolutePath(value: String) =
    value.startsWith('/') || WINDOWS_DRIVE_PATH.containsMatchIn(value) || value.startsWith("\\\\")

private fun fileLabel(value: String): String =
    value.replace('\\', '/').substringAfterLast('/').ifEmpty { value.replace('\\', '/') }

private fun decode(value: String): String =
    runCatching { java.net.URLDecoder.decode(value, Charsets.UTF_8.name()) }.getOrDefault(value)

private fun String.removeWindowsFileSlash(): String =
    if (matches(Regex("^/[A-Za-z]:[\\\\/].*"))) drop(1) else this
