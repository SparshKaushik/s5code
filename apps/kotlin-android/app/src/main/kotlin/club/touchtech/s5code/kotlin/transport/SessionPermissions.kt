package club.touchtech.s5code.kotlin.transport

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/** New servers report exact grants; old servers used broad parent scopes. */
internal fun SessionState.grants(permission: String): Boolean {
    if (!authenticated) return false
    permissions?.let { return permission in it }
    if (permission in scopes.orEmpty()) return true
    if (serverUpdateScope != null) return false
    val parent = when (permission) {
        "filesystem:read", "diagnostics:read" -> "orchestration:read"
        "terminal:read" -> "terminal:operate"
        "settings:write", "providers:manage", "environment:maintain", "preview:operate",
        "source-control:write", "filesystem:write" -> "orchestration:operate"
        else -> return false
    }
    return parent in scopes.orEmpty()
}

/** Guard writes at the transport boundary so every entry point gets the same denial. */
internal fun SessionState.requireRpcPermissions(method: String, payload: JsonElement) {
    // Unknown auth on an old server is left to that server's authorization.
    if (!authenticated || (permissions == null && scopes == null)) return
    val required = when (method) {
        "server.updateSettings" -> {
            val patch = (payload as? JsonObject)?.get("patch") as? JsonObject ?: payload as? JsonObject
            val keys = patch?.keys.orEmpty()
            val providerKeys = setOf("providers", "providerInstances", "usageLimitSources")
            buildList {
                if (keys.isEmpty() || keys.any { it !in providerKeys }) add("settings:write")
                if (keys.any { it in providerKeys }) add("providers:manage")
            }
        }
        "server.updateServer", "server.updateServerWithProgress", "server.commitDesktopUpdate" -> listOf("environment:maintain")
        "server.runStorageCleanup" -> listOf("settings:write")
        "server.updateProvider", "provider.auth.start", "provider.auth.complete", "provider.auth.respond",
        "provider.auth.cancel", "provider.auth.logout", "provider.consumeResetCredit" -> listOf("providers:manage")
        "projects.writeFile" -> listOf("filesystem:write")
        "projects.listEntries", "projects.readFile", "projects.searchContents", "projects.searchEntries",
        "filesystem.browse", "filesystem.getMetadata", "review.getDiffPreview", "review.getDiffFileContents" -> listOf("filesystem:read")
        "server.getUsageSummary", "server.getTraceDiagnostics", "server.getProcessDiagnostics" -> listOf("diagnostics:read")
        "sourceControl.cloneRepository", "sourceControl.publishRepository", "projectClone.start", "projectClone.cancel",
        "projectClone.retry", "git.runStackedAction", "git.preparePullRequestThread", "vcs.pull",
        "vcs.createWorktree", "vcs.removeWorktree", "vcs.createRef", "vcs.switchRef", "vcs.init",
        "pullRequests.runAction", "pullRequests.update", "pullRequests.comment", "pullRequests.updateComment",
        "pullRequests.submitReview", "pullRequests.replyToThread", "pullRequests.setThreadResolution",
        "pullRequests.setReaction", "pullRequests.setFilesViewed", "pullRequests.requestReviewers", "pullRequests.setLabels" -> listOf("source-control:write")
        "preview.open", "preview.navigate", "preview.resize", "preview.adjust", "preview.refresh", "preview.close" -> listOf("preview:operate")
        "terminal.write", "terminal.resize", "terminal.open", "terminal.restart", "terminal.close", "terminal.clear" -> listOf("terminal:operate")
        "orchestration.dispatchCommand", "orchestration.launchThread", "orchestration.stopWorkflow",
        "projects.mutate", "secrets.answerRequest", "mcpApps.callTool", "mcpApps.updateModelContext",
        "scheduledTasks.upsert", "scheduledTasks.setEnabled",
        "scheduledTasks.delete", "scheduledTasks.runNow", "scheduledTasks.rotateWebhookToken" -> listOf("orchestration:operate")
        else -> emptyList()
    }
    required.firstOrNull { !grants(it) }?.let {
        throw IllegalStateException("This connection does not grant $it. Pair again with the required permission.")
    }
}
