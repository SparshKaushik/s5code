package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLinkedThreadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackHeadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackLayerDto

/*
 * What a viewer may do to a pull request, ported from
 * `apps/mobile/src/features/pull-requests/` (`pullRequestActions.ts`,
 * `usePullRequestPermissions.ts`, `pullRequestStack.ts`,
 * `PullRequestMergeControls.tsx`, `pullRequestAgentTasks.ts`). Every rule is
 * pure so the screens stay dumb and the gateway can re-check a fresh read.
 */

/* ── Actions ─────────────────────────────────────────────────────────── */

/** `MERGE_METHODS` titles, in the host's merge-method order. */
val PULL_REQUEST_MERGE_METHOD_TITLES: Map<String, String> =
    linkedMapOf(
        "squash" to "Squash and merge",
        "merge" to "Create a merge commit",
        "rebase" to "Rebase and merge",
    )

fun mergeMethodTitle(method: String): String = PULL_REQUEST_MERGE_METHOD_TITLES[method] ?: method

val PULL_REQUEST_ACTIONS: List<String> =
    listOf(
        "merge", "ready", "draft", "close", "reopen", "update-branch", "enable-auto-merge",
        "disable-auto-merge", "revert", "delete-branch", "approve-workflows",
    )

/** `pullRequestActionLabel`, worded for the host (GitLab says merge request). */
fun pullRequestActionLabel(action: String, provider: String? = null): String {
    val singular = changeRequestTerms(provider).singular
    return when (action) {
        "merge" -> "Merge $singular"
        "ready" -> "Mark ready for review"
        "draft" -> "Convert to draft"
        "close" -> "Close $singular"
        "reopen" -> "Reopen $singular"
        "update-branch" -> "Update branch"
        "enable-auto-merge" -> "Enable auto-merge"
        "disable-auto-merge" -> "Disable auto-merge"
        "revert" -> "Create revert $singular"
        "delete-branch" -> "Delete branch"
        "approve-workflows" -> "Approve workflows"
        else -> action
    }
}

private fun PullRequestDetailDto.mergeCapable(method: String): Boolean =
    when (method) {
        "merge" -> mergeCapabilities.merge
        "squash" -> mergeCapabilities.squash
        "rebase" -> mergeCapabilities.rebase
        else -> false
    }

/** `allowedMergeMethods`: the host's methods this repository has switched on. */
fun PullRequestDetailDto.allowedMergeMethods(): List<String> = capabilities.mergeMethods.filter(::mergeCapable)

/** `allowedUpdateMethods`: host update methods the viewer may use. */
fun PullRequestDetailDto.allowedUpdateMethods(): List<String> =
    capabilities.updateMethods.orEmpty().filter { viewerPermissions.updateMethods?.contains(it) == true }

/** `canPerformPullRequestAction`. */
fun canPerformPullRequestAction(pr: PullRequestDetailDto, action: String): Boolean {
    if (action !in pr.capabilities.actions || action !in pr.viewerPermissions.actions) return false
    when (action) {
        "reopen" -> return pr.state == "closed"
        "revert" -> return pr.state == "merged"
        "delete-branch" -> return pr.state != "open" && pr.capabilities.deleteBranch == true
    }
    if (pr.state != "open") return false
    return when (action) {
        "update-branch" -> pr.allowedUpdateMethods().isNotEmpty()
        "approve-workflows" -> (pr.workflowApprovalsRequired ?: 0) > 0
        "ready" -> pr.isDraft
        "draft" -> !pr.isDraft
        "merge" -> !pr.isDraft && pr.mergeability != "conflicting" && pr.allowedMergeMethods().isNotEmpty()
        "enable-auto-merge" ->
            !pr.isDraft && pr.autoMergeEnabled == false && pr.allowedMergeMethods().isNotEmpty()
        "disable-auto-merge" -> pr.autoMergeEnabled == true
        else -> true
    }
}

const val MERGE_CONFLICT_REASON = "Resolve the branch conflicts before merging."

/** `mergeUnavailableReason`; null when a merge may be offered. */
fun mergeUnavailableReason(pr: PullRequestDetailDto, connectionAllowsWrites: Boolean): String? {
    val singular = changeRequestTerms(pr.provider).singular
    return when {
        pr.state == "merged" -> "This $singular has been merged."
        pr.state == "closed" -> "Reopen this $singular before merging."
        !connectionAllowsWrites -> "This environment connection is read-only."
        "merge" !in pr.capabilities.actions -> "This provider does not support merging here."
        "merge" !in pr.viewerPermissions.actions -> "Your account cannot merge this $singular."
        pr.isDraft -> "Mark this draft ready for review before merging."
        pr.mergeability == "conflicting" -> MERGE_CONFLICT_REASON
        pr.allowedMergeMethods().isEmpty() -> "This repository has no available merge methods."
        else -> null
    }
}

/** `mergeSummary`: the overview's Merge row value. */
fun mergeSummary(pr: PullRequestDetailDto, connectionAllowsWrites: Boolean): String {
    if (pr.state == "merged") return "Merged"
    if (pr.state == "closed") return "Closed"
    if (pr.isDraft) return "Draft"
    if (pr.mergeability == "conflicting") return "Has conflicts"
    if (!connectionAllowsWrites) return "Read-only connection"
    val allowed = pr.allowedMergeMethods()
    val method = pr.autoMergeMethod ?: (if ("squash" in allowed) "squash" else allowed.firstOrNull())
    val label = method?.replaceFirstChar { it.uppercase() } ?: "Merge"
    if (pr.autoMergeEnabled == true) return "$label · auto-merge on"
    if (mergeUnavailableReason(pr, connectionAllowsWrites) != null) return "Unavailable"
    return label + if (pr.autoMergeEnabled == false) " · auto-merge off" else ""
}

/** The merge method a merge would use: a pick, then the saved preference, then squash. */
fun resolveMergeMethod(allowed: List<String>, selected: String?, preferred: String?): String? {
    val wanted = selected ?: preferred ?: "squash"
    return when {
        wanted in allowed -> wanted
        "squash" in allowed -> "squash"
        else -> allowed.firstOrNull()
    }
}

/* ── Permissions ─────────────────────────────────────────────────────── */

/**
 * `usePullRequestPermissions`. [canWriteSourceControl] is the session grant
 * every pull request write needs (`source-control:write`).
 */
data class PullRequestPermissions(
    val connected: Boolean = false,
    val canInlineComment: Boolean = false,
    val canReply: Boolean = false,
    val canResolve: Boolean = false,
    val canWrite: Boolean = false,
    val canComment: Boolean = false,
    val canRequestReviewers: Boolean = false,
    val verdicts: List<String> = emptyList(),
    /** Edit the PR's title and description. */
    val canEditChangeRequest: Boolean = false,
    /** Allow or stop maintainer pushes to the PR's head branch. */
    val canEditMaintainerEdits: Boolean = false,
    /** React to the PR and its comments. */
    val canReact: Boolean = false,
    /** Add and remove labels. */
    val canManageLabels: Boolean = false,
    /** The PR [canEditComment] judges comments against; null before the detail loads. */
    private val editingSubject: PullRequestDetailDto? = null,
) {
    /** `canEditComment` from `usePullRequestPermissions`. */
    fun canEditComment(kind: String, authorLogin: String?, canEdit: Boolean?): Boolean =
        canWrite && editingSubject != null && canEditPullRequestComment(editingSubject, kind, authorLogin, canEdit)
}

fun pullRequestPermissions(
    pr: PullRequestDetailDto?,
    connected: Boolean,
    canWriteSourceControl: Boolean,
): PullRequestPermissions {
    val granted = connected && canWriteSourceControl
    val open = pr?.state == "open"
    val verdicts =
        if (granted && open && pr != null) {
            pr.capabilities.review.verdicts.filter { it in pr.viewerPermissions.verdicts }
        } else {
            emptyList()
        }
    return PullRequestPermissions(
        connected = connected,
        canInlineComment =
            granted && open && pr != null &&
                pr.capabilities.review.inlineComment &&
                pr.viewerPermissions.comment &&
                pr.capabilities.review.verdicts.any { it in pr.viewerPermissions.verdicts },
        canReply = granted && pr?.capabilities?.review?.reply == true && pr.viewerPermissions.comment,
        canResolve = granted && pr?.capabilities?.review?.resolve == true && pr.viewerPermissions.resolve,
        canWrite = granted,
        canComment = granted && pr?.capabilities?.comment == true && pr.viewerPermissions.comment,
        canRequestReviewers =
            granted && open && pr?.capabilities?.reviewers?.request == true && pr.viewerPermissions.requestReviewers,
        verdicts = verdicts,
        canEditChangeRequest = granted && pr != null && canEditPullRequestChangeRequest(pr),
        canEditMaintainerEdits = granted && pr != null && canEditPullRequestMaintainerEdits(pr),
        canReact = granted && pr?.capabilities?.reactions == true,
        canManageLabels = granted && pr != null && canManagePullRequestLabels(pr),
        editingSubject = pr,
    )
}

/* ── Stacks ──────────────────────────────────────────────────────────── */

/** `pullRequestStackMerge`: the layers a merge through [number] takes with it. */
data class PullRequestStackMergeScope(
    /** One-based position of [number] in the stack; 0 when it is not in it. */
    val position: Int,
    val layers: List<PullRequestStackLayerDto>,
    val heads: List<PullRequestStackHeadDto>,
    val reason: String?,
)

fun pullRequestStackMerge(stack: PullRequestStackDto, number: Int): PullRequestStackMergeScope {
    val position = stack.layers.indexOfFirst { it.number == number }
    val layers = stack.layers.take(position + 1).filter { it.state != "merged" }
    val heads = layers.mapNotNull { layer -> layer.headSha?.let { PullRequestStackHeadDto(layer.number, it) } }
    val reason =
        when {
            position < 0 -> "This pull request is no longer in the stack. Refresh it."
            stack.layers[position].state != "open" || layers.isEmpty() ->
                "There are no open layers to merge through this pull request."
            layers.any { it.state != "open" } -> "Reopen the closed layers before merging this stack."
            layers.any { it.isDraft == true } -> "Mark every layer being merged ready for review."
            heads.size != layers.size -> "Refresh the stack to load its latest revisions before merging."
            else -> null
        }
    return PullRequestStackMergeScope(position + 1, layers, heads, reason)
}

/** `singlePullRequestMergeAllowed`: a lone merge must know the PR is in no stack. */
fun singlePullRequestMergeAllowed(supportsStackActions: Boolean, hasStack: Boolean, isFresh: Boolean): Boolean =
    !supportsStackActions || (!hasStack && isFresh)

const val CHECKING_STACK_REASON = "Checking stack membership…"

/** What the merge button says and whether it may run (`PullRequestMergeControls`). */
data class PullRequestMergeState(
    val method: String?,
    val scope: PullRequestStackMergeScope?,
    val reason: String?,
    val canMerge: Boolean,
    val label: String,
    /** enable-auto-merge | disable-auto-merge */
    val autoAction: String,
    val canAutoMerge: Boolean,
    val checkingStack: Boolean,
    /** The reason worth showing under the button; the button already says the rest. */
    val visibleReason: String?,
)

fun pullRequestMergeState(
    pr: PullRequestDetailDto,
    permissions: PullRequestPermissions,
    method: String?,
    stack: PullRequestStackDto?,
    stackFresh: Boolean,
    stackFailed: Boolean,
    supportsStackActions: Boolean,
    busy: Boolean,
): PullRequestMergeState {
    val scope = stack?.let { pullRequestStackMerge(it, pr.number) }
    val canMergeSingle = singlePullRequestMergeAllowed(supportsStackActions, stack != null, stackFresh)
    val checkingStack = permissions.connected && supportsStackActions && !stackFresh && !stackFailed
    val reason =
        (if (!permissions.connected) "Reconnect to merge." else mergeUnavailableReason(pr, permissions.canWrite))
            ?: when {
                stack != null ->
                    when {
                        !supportsStackActions -> "This environment can display stacks but cannot merge them."
                        !stackFresh -> "Refresh the stack before merging."
                        else -> scope?.reason
                    }
                !canMergeSingle ->
                    if (stackFailed) "Couldn't check stack membership. Refresh before merging."
                    else CHECKING_STACK_REASON
                else -> null
            }
    val label =
        when {
            method == null -> "Merge unavailable"
            scope != null && scope.layers.size > 1 -> "${mergeMethodTitle(method)} stack (${scope.layers.size})"
            else -> mergeMethodTitle(method)
        }
    val autoAction = if (pr.autoMergeEnabled == true) "disable-auto-merge" else "enable-auto-merge"
    val canAutoMerge =
        !busy &&
            (autoAction == "disable-auto-merge" || (stack == null && canMergeSingle)) &&
            permissions.canWrite &&
            canPerformPullRequestAction(pr, autoAction)
    val visibleReason =
        reason?.takeIf {
            permissions.connected &&
                it != MERGE_CONFLICT_REASON &&
                it != CHECKING_STACK_REASON &&
                !(checkingStack && it == "Refresh the stack before merging.")
        }
    return PullRequestMergeState(
        method = method,
        scope = scope,
        reason = reason,
        canMerge = !busy && method != null && reason == null,
        label = label,
        autoAction = autoAction,
        canAutoMerge = canAutoMerge,
        checkingStack = checkingStack,
        visibleReason = visibleReason,
    )
}

/** The merge confirmation's title and message. */
fun pullRequestMergeConfirmation(
    action: String,
    pr: PullRequestDetailDto,
    method: String?,
    scope: PullRequestStackMergeScope?,
    stackBase: String?,
): Pair<String, String> {
    val title =
        when (action) {
            "merge" ->
                if (scope != null && scope.layers.size > 1) "Merge ${scope.layers.size} stack layers?"
                else "Merge pull request?"
            "enable-auto-merge" -> "Enable auto-merge?"
            else -> "Disable auto-merge?"
        }
    val message =
        when {
            action == "merge" && scope != null ->
                "Merge ${scope.layers.joinToString(" → ") { "#${it.number}" }} into $stackBase using $method. " +
                    "Layers above #${pr.number} stay open."
            action == "enable-auto-merge" ->
                "Merge #${pr.number} using $method when its requirements are met. This may happen immediately."
            action == "disable-auto-merge" -> "Stop the provider from automatically merging this pull request."
            else -> "Merge #${pr.number} into ${pr.baseBranch} using $method."
        }
    return title to message
}

/**
 * The merge-time re-check against a fresh read: the method chosen on a stale
 * page must still be allowed, or the merge is refused.
 */
fun freshMergeMethod(fresh: PullRequestDetailDto, method: String): String {
    if (!canPerformPullRequestAction(fresh, "merge") || method !in fresh.allowedMergeMethods()) {
        throw IllegalStateException("The pull request changed. Refresh before merging.")
    }
    return method
}

/** The branch row's title. */
fun pullRequestBranchTitle(pr: PullRequestDetailDto): String =
    when (pr.mergeability) {
        "conflicting" -> "Conflicts with ${pr.baseBranch}"
        "mergeable" -> "No conflicts with ${pr.baseBranch}"
        else -> "Branch"
    }

/**
 * The status writes a list row offers from its summary; the fresh detail is
 * read before one runs, since the row may be stale.
 */
fun pullRequestListRowActions(state: String, isDraft: Boolean): List<String> =
    when (state) {
        "open" -> listOf(if (isDraft) "ready" else "draft", "close")
        "closed" -> listOf("reopen")
        else -> emptyList()
    }

/** Confirmation copy for the status menu's writes; null runs without asking. */
fun pullRequestActionConfirmation(action: String, pr: PullRequestDetailDto): Pair<String, String>? {
    val singular = changeRequestTerms(pr.provider).singular
    return when (action) {
        "close" -> pullRequestActionLabel(action, pr.provider) to "Close this $singular without merging its changes?"
        "revert" -> pullRequestActionLabel(action, pr.provider) to "Open a new $singular that reverses these changes?"
        "approve-workflows" ->
            "Approve workflows" to "Allow this $singular's workflows to run? Review the workflow changes first."
        "delete-branch" ->
            "Delete branch" to
                "Delete ${pr.headBranch} from ${pr.headRepositoryNameWithOwner ?: pr.repository}? " +
                "Local branches and worktrees are not changed."
        else -> null
    }
}

/* ── Agent tasks ─────────────────────────────────────────────────────── */

enum class PullRequestAgentTask(val id: String, val title: String) {
    Ask("ask", "Ask a question"),
    Explain("explain", "Explain this PR"),
    Feedback("feedback", "Address feedback"),
    Conflicts("conflicts", "Resolve conflicts"),
    Watch("watch", "Watch PR"),
}

/** `pullRequestAgentPrompt`, verbatim. */
fun pullRequestAgentPrompt(task: PullRequestAgentTask): String =
    when (task) {
        PullRequestAgentTask.Ask -> ""
        PullRequestAgentTask.Explain ->
            "Explain this pull request. Read the diff and walk through its purpose, the changes, the risks, " +
                "and what I should read closely before approving. Do not change any code."
        PullRequestAgentTask.Feedback ->
            "Address the actionable feedback and failing checks on this pull request. Fetch the latest review " +
                "threads and check results, skip resolved feedback, and verify each finding before changing code. " +
                "First confirm this thread is on the PR's branch; if it isn't, prepare the PR in its own worktree. " +
                "Keep the fixes focused and run the relevant checks."
        PullRequestAgentTask.Conflicts ->
            "Resolve this pull request's conflicts with its base branch. First confirm this thread is on the " +
                "PR's branch; if it isn't, prepare the PR in its own worktree. Bring it up to date using this " +
                "repository's convention, preserve the intent of both sides, and run the relevant checks before pushing."
        PullRequestAgentTask.Watch ->
            "Watch this pull request for check results, new feedback, and conflicts using T3's pull request watch. " +
                "End the turn once the watch is armed. When it wakes you, assess what changed and report what " +
                "needs attention."
    }

data class PullRequestAgentAction(val task: PullRequestAgentTask, val disabled: Boolean)

/** `pullRequestAgentActions`. */
fun pullRequestAgentActions(pr: PullRequestDetailDto, canWatch: Boolean): List<PullRequestAgentAction> =
    listOf(
        PullRequestAgentAction(PullRequestAgentTask.Ask, disabled = false),
        PullRequestAgentAction(PullRequestAgentTask.Explain, disabled = false),
        PullRequestAgentAction(PullRequestAgentTask.Feedback, disabled = pr.state != "open"),
        PullRequestAgentAction(
            PullRequestAgentTask.Conflicts,
            disabled = pr.state != "open" || pr.mergeability != "conflicting",
        ),
        PullRequestAgentAction(PullRequestAgentTask.Watch, disabled = pr.state != "open" || !canWatch),
    )

/** `pullRequestAgentThread`: the named thread, else the only one. */
fun pullRequestAgentThread(threads: List<PullRequestLinkedThreadDto>, threadId: String? = null): PullRequestLinkedThreadDto? =
    if (threadId != null) threads.firstOrNull { it.id == threadId } else threads.singleOrNull()

/**
 * `appendPullRequestAgentPrompt`: swaps the prompt a previous task appended
 * for this one, keeping whatever the user wrote around it.
 */
fun appendPullRequestAgentPrompt(text: String, task: PullRequestAgentTask, previousPrompt: String? = null): String {
    val prompt = pullRequestAgentPrompt(task)
    val kept =
        when {
            previousPrompt.isNullOrEmpty() -> text
            text == previousPrompt -> ""
            text.trimEnd().endsWith("\n\n$previousPrompt") ->
                text.trimEnd().dropLast(previousPrompt.length + 2)
            else -> text
        }
    if (prompt.isEmpty() || kept.trimEnd().endsWith(prompt)) return kept
    return if (kept.isNotBlank()) "${kept.trimEnd()}\n\n$prompt" else prompt
}

/** The bottom bar's primary label. */
fun pullRequestPrimaryActionLabel(reviewing: Boolean, pendingComments: Int, threads: Int): String =
    when {
        reviewing && pendingComments > 0 -> "Submit review · $pendingComments"
        threads > 0 -> "Continue thread"
        else -> "Work on PR"
    }
