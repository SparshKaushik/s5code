package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.transport.WsMethods
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** `WORKSPACE_BASENAME_LOOKUP_LIMIT` in `workspaceBasenameLookup.ts`. */
internal const val WORKSPACE_BASENAME_LOOKUP_LIMIT = 25

/** One `projects.searchEntries` row; [kind] is `file` or `directory`. */
internal data class WorkspaceEntryCandidate(val path: String, val kind: String)

/**
 * `needsWorkspaceBasenameLookup`: an agent often links a bare file name
 * (`AGENTS.md`) that lives somewhere below the root, so a link with no
 * directory part is worth a search before it opens as a root path.
 */
internal fun needsWorkspaceBasenameLookup(relativePath: String): Boolean {
    val trimmed = relativePath.trim()
    return trimmed.isNotEmpty() && trimmed != "." && trimmed != ".." &&
        '/' !in trimmed && '\\' !in trimmed
}

/**
 * `pickWorkspaceBasenameMatch`: the first file whose name matches exactly, else
 * the only case-insensitive one. Several case-folded matches are ambiguous.
 */
internal fun pickWorkspaceBasenameMatch(
    basename: String,
    entries: List<WorkspaceEntryCandidate>,
): String? {
    val target = basename.trim().takeIf(String::isNotEmpty) ?: return null
    val files = entries.filter { it.kind == "file" }
    files.firstOrNull { basenameOf(it.path) == target }?.let { return it.path }
    return files.filter { basenameOf(it.path).equals(target, ignoreCase = true) }
        .singleOrNull()?.path
}

private fun basenameOf(path: String): String =
    path.substring(maxOf(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)

// One counter for every transcript: the newest tap wins, so a slow lookup for
// an earlier link cannot navigate over the one the user tapped since.
private val latestFileLinkLookup = AtomicLong()

/**
 * The route suffix a transcript file link opens at, following `onMarkdownLinkPress`
 * in RN's `ThreadFeed.tsx`. [path] is what the Markdown renderer resolved:
 * workspace-relative, with a trailing `/` when the link names a directory.
 * A bare file name is looked up in the workspace and opens the match unless the
 * root already holds a directory by that name. Null when a newer tap superseded
 * this one.
 */
internal suspend fun transcriptFileLinkSuffix(
    gateway: WorkspaceGateway,
    environmentId: EnvironmentId,
    workspaceRoot: String?,
    path: String,
): String? {
    val claim = latestFileLinkLookup.incrementAndGet()
    val target =
        if (workspaceRoot.isNullOrBlank() || path.endsWith('/') || !needsWorkspaceBasenameLookup(path)) {
            path
        } else {
            resolveBasenameLink(gateway, environmentId, workspaceRoot, path)
        }
    if (claim != latestFileLinkLookup.get()) return null
    return club.touchtech.s5code.kotlin.app.Routes.fileLinkSuffix(target)
}

private suspend fun resolveBasenameLink(
    gateway: WorkspaceGateway,
    environmentId: EnvironmentId,
    workspaceRoot: String,
    relativePath: String,
): String =
    try {
        val entries =
            (gateway.environmentRequest(
                environmentId,
                WsMethods.ProjectsSearchEntries,
                buildJsonObject {
                    put("cwd", workspaceRoot)
                    put("query", relativePath)
                    put("kind", "file")
                    put("limit", WORKSPACE_BASENAME_LOOKUP_LIMIT)
                },
            )["entries"] as? JsonArray).orEmpty().mapNotNull { raw ->
                val entry = raw as? JsonObject ?: return@mapNotNull null
                val entryPath = entry["path"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
                WorkspaceEntryCandidate(entryPath, entry["kind"]?.jsonPrimitive?.contentOrNull ?: "file")
            }
        val match = pickWorkspaceBasenameMatch(relativePath, entries)
        if (match == null || match == relativePath) {
            relativePath
        } else {
            // A root directory of that name is what the link meant; a missing
            // or regular root entry yields to the file the search found.
            val metadata =
                gateway.environmentRequest(
                    environmentId,
                    "filesystem.getMetadata",
                    buildJsonObject {
                        putJsonArray("paths") {
                            add(JsonPrimitive("${workspaceRoot.trimEnd('/', '\\')}/$relativePath"))
                        }
                    },
                )
            val original = (metadata["entries"] as? JsonArray)?.firstOrNull() as? JsonObject
            if (original?.get("kind")?.jsonPrimitive?.contentOrNull == "directory") relativePath else match
        }
    } catch (cancellation: CancellationException) {
        throw cancellation
    } catch (_: Exception) {
        relativePath
    }
