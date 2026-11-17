package club.touchtech.s5code.kotlin.feature.connections

internal fun permissionLabel(permission: String): String = when (permission) {
    "orchestration:read" -> "read threads"
    "orchestration:operate" -> "work on threads"
    "settings:write" -> "change server settings"
    "providers:manage" -> "manage providers"
    "environment:maintain" -> "maintain the server"
    "preview:operate" -> "control the browser"
    "diagnostics:read" -> "read diagnostics and usage"
    "terminal:read" -> "read terminal output"
    "terminal:operate" -> "control terminals"
    "source-control:write" -> "change repositories"
    "filesystem:read" -> "read files"
    "filesystem:write" -> "write files"
    "access:read" -> "view paired devices"
    "access:write" -> "manage paired devices"
    "relay:read" -> "view cloud connections"
    "relay:write" -> "manage cloud connections"
    else -> permission.substringBefore(':').replace('-', ' ')
}
