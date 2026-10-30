package club.touchtech.s5code.kotlin.app

import club.touchtech.s5code.kotlin.data.PullRequestAgentTask
import club.touchtech.s5code.kotlin.data.appendPullRequestAgentPrompt
import club.touchtech.s5code.kotlin.data.formatComposerContextReference
import club.touchtech.s5code.kotlin.data.pullRequestAgentPrompt
import club.touchtech.s5code.kotlin.data.pullRequestComposerContext
import club.touchtech.s5code.kotlin.model.ComposerContextRecord
import club.touchtech.s5code.kotlin.model.ComposerPullRequestCandidate
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLinkedThreadDto

/** Where "Work on PR" left its draft; the caller navigates there. */
sealed interface PullRequestWorkDestination {
    data class Thread(val environmentId: String, val threadId: String) : PullRequestWorkDestination

    /** The single new-task draft, now on the PR's head branch. */
    data object NewTask : PullRequestWorkDestination
}

/**
 * Whether starting fresh work on [pr] would overwrite text the user typed
 * into the single new-task draft for something else. RN gives every task its
 * own draft, so it never has to ask.
 */
fun AppStore.newTaskDraftWouldBeReplaced(environmentId: String, pr: PullRequestDetailDto): Boolean =
    newTaskDraftWouldBeReplaced(draft.value, environmentId, pr.url)

/** Pure core of [AppStore.newTaskDraftWouldBeReplaced]; "same PR" matches [preparePullRequestWork]. */
internal fun newTaskDraftWouldBeReplaced(draft: NewTaskDraft, environmentId: String, pullRequestUrl: String): Boolean =
    draft.hasContent &&
        !(draft.pullRequestReference == pullRequestUrl && draft.environmentId.value == environmentId)

/**
 * `usePullRequestThreads.ask`: attaches [pr] as composer context (once per
 * file) and appends [task]'s prompt, swapping the prompt an earlier task left.
 * With a linked [thread] the thread's draft gets it; otherwise the new-task
 * draft is pointed at a fresh worktree on the PR's head branch, which Send
 * checks out with `git.preparePullRequestThread`.
 *
 * Throws with a message for "Couldn't open a thread".
 */
fun AppStore.preparePullRequestWork(
    environmentId: String,
    pr: PullRequestDetailDto,
    thread: PullRequestLinkedThreadDto?,
    task: PullRequestAgentTask,
    filePath: String?,
    connected: Boolean,
    canCheckout: Boolean,
): PullRequestWorkDestination {
    if (connected && thread == null && !canCheckout) {
        throw IllegalStateException("Starting work from this PR requires source control write access.")
    }
    val context =
        pullRequestComposerContext(
            ComposerPullRequestCandidate(
                projectId = pr.projectId,
                repository = pr.repository,
                number = pr.number,
                title = pr.title,
                url = pr.url,
                headBranch = pr.headBranch,
                baseBranch = pr.baseBranch,
                state = pr.state,
                isDraft = pr.isDraft,
                updatedAt = pr.updatedAt,
            ),
            filePath = filePath,
        )
    fun withContext(text: String, records: List<ComposerContextRecord>): Pair<String, List<ComposerContextRecord>> {
        val attached =
            records.any {
                it.kind == "review-comment" && it.pullRequestUrl == pr.url && it.filePath == context.filePath
            }
        return if (attached) text to records else "$text${formatComposerContextReference(context)} " to records + context
    }
    val prompt = pullRequestAgentPrompt(task)
    if (thread != null) {
        val key = "thread:$environmentId/${thread.id}"
        val current = threadDraft(environmentId, thread.id)
        val (text, records) = withContext(current.text, current.contextRecords)
        setThreadDraftWithContext(
            environmentId,
            thread.id,
            appendPullRequestAgentPrompt(text, task, pullRequestAgentPrompts[key]),
            records,
        )
        pullRequestAgentPrompts[key] = prompt
        return PullRequestWorkDestination.Thread(environmentId, thread.id)
    }
    val key = "new-task"
    updateDraft(setsPullRequest = true) { current ->
        // Continuing the same PR's draft keeps what was written; anything else starts over.
        val same = current.pullRequestReference == pr.url && current.environmentId.value == environmentId
        if (!same) pullRequestAgentPrompts.remove(key)
        val (text, records) =
            withContext(if (same) current.prompt else "", if (same) current.contextRecords else emptyList())
        current.copy(
            environmentId = EnvironmentId(environmentId),
            projectKey = pr.projectId,
            prompt = appendPullRequestAgentPrompt(text, task, pullRequestAgentPrompts[key]),
            attachments = if (same) current.attachments else emptyList(),
            contextRecords = records,
            branch = pr.headBranch,
            workspaceMode = WorkspaceMode.NewWorktree,
            worktreePath = null,
            pullRequestReference = pr.url,
            cloning = false,
        )
    }
    pullRequestAgentPrompts[key] = prompt
    return PullRequestWorkDestination.NewTask
}
