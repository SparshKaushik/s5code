package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestMergeCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewCapabilitiesDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackHeadDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackLayerDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestViewerPermissionsDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Ports of the mobile `pullRequestActions`, `pullRequestStack` and `pullRequestAgentTasks` tests. */
class PullRequestActionsTest {
    private val allActions = PULL_REQUEST_ACTION_LABELS.keys.toList()

    private fun detail(
        state: String = "open",
        isDraft: Boolean = false,
        mergeability: String = "mergeable",
        autoMergeEnabled: Boolean? = false,
        capabilities: PullRequestCapabilitiesDto =
            PullRequestCapabilitiesDto(
                actions = allActions,
                mergeMethods = listOf("merge", "squash", "rebase"),
                updateMethods = listOf("merge", "rebase"),
            ),
        viewerPermissions: PullRequestViewerPermissionsDto =
            PullRequestViewerPermissionsDto(actions = allActions, updateMethods = listOf("merge")),
        mergeCapabilities: PullRequestMergeCapabilitiesDto =
            PullRequestMergeCapabilitiesDto(merge = true, squash = true, rebase = false),
        workflowApprovalsRequired: Int? = null,
    ) =
        PullRequestDetailDto(
            number = 2,
            baseBranch = "main",
            state = state,
            isDraft = isDraft,
            mergeability = mergeability,
            autoMergeEnabled = autoMergeEnabled,
            capabilities = capabilities,
            viewerPermissions = viewerPermissions,
            mergeCapabilities = mergeCapabilities,
            workflowApprovalsRequired = workflowApprovalsRequired,
        )

    /* ── Actions ─────────────────────────────────────────────────────── */

    @Test
    fun `offers branch updates only through methods the provider and account allow`() {
        assertEquals(listOf("merge"), detail().allowedUpdateMethods())
        assertTrue(canPerformPullRequestAction(detail(), "update-branch"))
        val unsupported = detail(viewerPermissions = PullRequestViewerPermissionsDto(actions = allActions))
        assertEquals(emptyList<String>(), unsupported.allowedUpdateMethods())
        assertFalse(canPerformPullRequestAction(unsupported, "update-branch"))
    }

    @Test
    fun `offers workflow approval only for reported pending approvals`() {
        assertFalse(canPerformPullRequestAction(detail(), "approve-workflows"))
        assertTrue(canPerformPullRequestAction(detail(workflowApprovalsRequired = 2), "approve-workflows"))
        assertFalse(canPerformPullRequestAction(detail(workflowApprovalsRequired = 0), "approve-workflows"))
    }

    @Test
    fun `offers only merge methods allowed by both the provider and repository`() {
        assertEquals(listOf("merge", "squash"), detail().allowedMergeMethods())
        val pr = detail(capabilities = PullRequestCapabilitiesDto(actions = allActions, mergeMethods = listOf("merge")))
        assertEquals(listOf("merge"), pr.allowedMergeMethods())
        assertEquals("Merge · auto-merge off", mergeSummary(pr, true))
    }

    @Test
    fun `requires both provider support and account permission`() {
        assertFalse(canPerformPullRequestAction(detail(viewerPermissions = PullRequestViewerPermissionsDto()), "merge"))
        assertFalse(
            canPerformPullRequestAction(
                detail(capabilities = PullRequestCapabilitiesDto(mergeMethods = listOf("merge"))),
                "merge",
            )
        )
        assertTrue(mergeUnavailableReason(detail(), false)!!.contains("read-only"))
    }

    @Test
    fun `turns drafts into ready PRs before allowing a merge`() {
        val draft = detail(isDraft = true)
        assertFalse(canPerformPullRequestAction(draft, "merge"))
        assertTrue(canPerformPullRequestAction(draft, "ready"))
        assertFalse(canPerformPullRequestAction(draft, "draft"))
        assertFalse(canPerformPullRequestAction(detail(), "ready"))
        assertTrue(canPerformPullRequestAction(detail(), "draft"))
    }

    @Test
    fun `blocks a conflicting PR's merge and explains why`() {
        val pr = detail(mergeability = "conflicting")
        assertFalse(canPerformPullRequestAction(pr, "merge"))
        assertEquals(MERGE_CONFLICT_REASON, mergeUnavailableReason(pr, true))
        assertEquals("Has conflicts", mergeSummary(pr, true))
    }

    @Test
    fun `offers the reverse lifecycle action after a close, and revert after a merge`() {
        val closed = detail(state = "closed")
        assertFalse(canPerformPullRequestAction(closed, "close"))
        assertFalse(canPerformPullRequestAction(closed, "merge"))
        assertTrue(canPerformPullRequestAction(closed, "reopen"))
        assertFalse(canPerformPullRequestAction(detail(), "reopen"))
        val merged = detail(state = "merged")
        assertFalse(canPerformPullRequestAction(merged, "reopen"))
        assertTrue(canPerformPullRequestAction(merged, "revert"))
        assertEquals("Merged", mergeSummary(merged, true))
    }

    @Test
    fun `offers disable auto-merge only when armed, and enable only when explicitly off`() {
        val armed = detail(autoMergeEnabled = true)
        assertFalse(canPerformPullRequestAction(armed, "enable-auto-merge"))
        assertTrue(canPerformPullRequestAction(armed, "disable-auto-merge"))
        assertTrue(canPerformPullRequestAction(detail(), "enable-auto-merge"))
        assertFalse(canPerformPullRequestAction(detail(), "disable-auto-merge"))
        val unknown = detail(autoMergeEnabled = null)
        assertFalse(canPerformPullRequestAction(unknown, "enable-auto-merge"))
        assertEquals("Squash", mergeSummary(unknown, true))
    }

    @Test
    fun `offers deleting the source branch only once finished on a host that supports it`() {
        val capabilities = detail().capabilities.copy(deleteBranch = true)
        assertFalse(canPerformPullRequestAction(detail(capabilities = capabilities), "delete-branch"))
        for (state in listOf("merged", "closed")) {
            assertTrue(canPerformPullRequestAction(detail(state = state, capabilities = capabilities), "delete-branch"))
            assertFalse(canPerformPullRequestAction(detail(state = state), "delete-branch"))
        }
        val refused =
            detail(state = "merged", capabilities = capabilities, viewerPermissions = PullRequestViewerPermissionsDto())
        assertFalse(canPerformPullRequestAction(refused, "delete-branch"))
    }

    @Test
    fun `cannot merge when the repository has disabled every offered method`() {
        val pr = detail(mergeCapabilities = PullRequestMergeCapabilitiesDto())
        assertFalse(canPerformPullRequestAction(pr, "merge"))
        assertFalse(canPerformPullRequestAction(pr, "enable-auto-merge"))
        assertTrue(mergeUnavailableReason(pr, true)!!.contains("no available merge methods"))
    }

    @Test
    fun `resolves the merge method from the pick, then the saved preference, then squash`() {
        val allowed = listOf("merge", "squash")
        assertEquals("merge", resolveMergeMethod(allowed, "merge", "squash"))
        assertEquals("merge", resolveMergeMethod(allowed, null, "merge"))
        assertEquals("squash", resolveMergeMethod(allowed, null, "rebase"))
        assertEquals("rebase", resolveMergeMethod(listOf("rebase"), null, null))
        assertNull(resolveMergeMethod(emptyList(), "merge", null))
    }

    @Test
    fun `refuses a merge whose method went away since the page loaded`() {
        assertEquals("squash", freshMergeMethod(detail(), "squash"))
        val changed = runCatching { freshMergeMethod(detail(), "rebase") }
        assertEquals("The pull request changed. Refresh before merging.", changed.exceptionOrNull()?.message)
    }

    @Test
    fun `list rows offer the status writes their summary allows`() {
        assertEquals(listOf("draft", "close"), pullRequestListRowActions("open", isDraft = false))
        assertEquals(listOf("ready", "close"), pullRequestListRowActions("open", isDraft = true))
        assertEquals(listOf("reopen"), pullRequestListRowActions("closed", isDraft = false))
        assertEquals(emptyList<String>(), pullRequestListRowActions("merged", isDraft = false))
    }

    /* ── Permissions ─────────────────────────────────────────────────── */

    @Test
    fun `permissions need a connection, the write grant and the host's say`() {
        val pr =
            detail().copy(
                capabilities =
                    detail().capabilities.copy(
                        comment = true,
                        review =
                            PullRequestReviewCapabilitiesDto(
                                inlineComment = true,
                                reply = true,
                                resolve = true,
                                verdicts = listOf("approve", "request-changes", "comment"),
                            ),
                    ),
                viewerPermissions =
                    detail().viewerPermissions.copy(comment = true, resolve = true, verdicts = listOf("comment", "approve")),
            )
        val granted = pullRequestPermissions(pr, connected = true, canWriteSourceControl = true)
        assertTrue(granted.canInlineComment && granted.canReply && granted.canResolve && granted.canComment)
        assertEquals(listOf("approve", "comment"), granted.verdicts)
        val readOnly = pullRequestPermissions(pr, connected = true, canWriteSourceControl = false)
        assertFalse(readOnly.canWrite || readOnly.canComment || readOnly.canInlineComment)
        assertEquals(emptyList<String>(), readOnly.verdicts)
        val offline = pullRequestPermissions(pr, connected = false, canWriteSourceControl = true)
        assertFalse(offline.canWrite)
        val merged = pullRequestPermissions(pr.copy(state = "merged"), connected = true, canWriteSourceControl = true)
        assertEquals(emptyList<String>(), merged.verdicts)
        assertFalse(merged.canInlineComment)
        assertTrue(merged.canComment)
    }

    /* ── Stacks ──────────────────────────────────────────────────────── */

    private val stack =
        PullRequestStackDto(
            id = "stack",
            number = 10,
            base = "main",
            layers = (1..3).map { PullRequestStackLayerDto(number = it, headSha = "sha-$it", headBranch = "layer-$it", isDraft = false) },
        )

    private fun PullRequestStackDto.withLayer(number: Int, transform: (PullRequestStackLayerDto) -> PullRequestStackLayerDto) =
        copy(layers = layers.map { if (it.number == number) transform(it) else it })

    @Test
    fun `merges only the prefix through the selected PR, in host order`() {
        val scope = pullRequestStackMerge(stack, 2)
        assertEquals(2, scope.position)
        assertEquals(listOf(PullRequestStackHeadDto(1, "sha-1"), PullRequestStackHeadDto(2, "sha-2")), scope.heads)
        assertNull(scope.reason)
        assertEquals(3, pullRequestStackMerge(stack, 3).heads.size)
    }

    @Test
    fun `leaves merged layers out of the expected heads`() {
        val updated = stack.withLayer(1) { it.copy(state = "merged", headSha = null) }
        assertEquals(listOf(PullRequestStackHeadDto(2, "sha-2")), pullRequestStackMerge(updated, 2).heads)
        assertNull(pullRequestStackMerge(updated, 2).reason)
        assertTrue(pullRequestStackMerge(updated, 1).reason!!.contains("no open layers"))
    }

    @Test
    fun `blocks closed and draft dependencies, but not ones above the target`() {
        val closed = stack.withLayer(2) { it.copy(state = "closed") }
        assertTrue(pullRequestStackMerge(closed, 3).reason!!.contains("Reopen"))
        assertNull(pullRequestStackMerge(closed, 1).reason)
        val draft = stack.withLayer(2) { it.copy(isDraft = true) }
        assertTrue(pullRequestStackMerge(draft, 3).reason!!.contains("ready for review"))
        assertNull(pullRequestStackMerge(draft, 1).reason)
    }

    @Test
    fun `blocks missing revisions and a PR removed from the stack`() {
        val unknown = stack.copy(layers = stack.layers.map { it.copy(headSha = null) })
        assertTrue(pullRequestStackMerge(unknown, 2).reason!!.contains("latest revisions"))
        assertTrue(pullRequestStackMerge(stack, 99).reason!!.contains("no longer in the stack"))
    }

    @Test
    fun `waits for stack discovery before a single merge on servers with stack actions`() {
        assertFalse(singlePullRequestMergeAllowed(supportsStackActions = true, hasStack = false, isFresh = false))
        assertTrue(singlePullRequestMergeAllowed(supportsStackActions = true, hasStack = false, isFresh = true))
        assertFalse(singlePullRequestMergeAllowed(supportsStackActions = true, hasStack = true, isFresh = true))
        assertTrue(singlePullRequestMergeAllowed(supportsStackActions = false, hasStack = false, isFresh = false))
    }

    private val writer = PullRequestPermissions(connected = true, canWrite = true)

    private fun mergeState(
        pr: PullRequestDetailDto = detail(),
        permissions: PullRequestPermissions = writer,
        stack: PullRequestStackDto? = null,
        fresh: Boolean = true,
        failed: Boolean = false,
        supportsStackActions: Boolean = true,
    ) = pullRequestMergeState(pr, permissions, "squash", stack, fresh, failed, supportsStackActions, busy = false)

    @Test
    fun `merge state labels a stack merge and blocks until the stack is fresh`() {
        val ready = mergeState()
        assertTrue(ready.canMerge)
        assertEquals("Squash and merge", ready.label)
        val layered = mergeState(stack = stack, pr = detail().copy(number = 3))
        assertEquals("Squash and merge stack (3)", layered.label)
        assertTrue(layered.canMerge)
        val stale = mergeState(stack = stack, fresh = false)
        assertEquals("Refresh the stack before merging.", stale.reason)
        assertFalse(stale.canMerge)
        // Still looking: the button says why, so nothing shows under it.
        assertNull(stale.visibleReason)
        val checking = mergeState(fresh = false)
        assertEquals(CHECKING_STACK_REASON, checking.reason)
        assertTrue(checking.checkingStack)
        assertNull(checking.visibleReason)
        val lookupFailed = mergeState(fresh = false, failed = true)
        assertEquals("Couldn't check stack membership. Refresh before merging.", lookupFailed.visibleReason)
        assertEquals(
            "This environment can display stacks but cannot merge them.",
            mergeState(stack = stack, supportsStackActions = false).reason,
        )
    }

    @Test
    fun `merge state asks to reconnect when offline and offers auto-merge only for a lone PR`() {
        val offline = mergeState(permissions = PullRequestPermissions())
        assertEquals("Reconnect to merge.", offline.reason)
        assertNull(offline.visibleReason)
        assertTrue(mergeState().canAutoMerge)
        assertFalse(mergeState(stack = stack).canAutoMerge)
        val armed = mergeState(pr = detail(autoMergeEnabled = true), stack = stack)
        assertEquals("disable-auto-merge", armed.autoAction)
        assertTrue(armed.canAutoMerge)
    }

    @Test
    fun `merge confirmation names the layers a stack merge takes`() {
        val scope = pullRequestStackMerge(stack, 2)
        val (title, message) = pullRequestMergeConfirmation("merge", detail(), "squash", scope, "main")
        assertEquals("Merge 2 stack layers?", title)
        assertEquals("Merge #1 → #2 into main using squash. Layers above #2 stay open.", message)
        assertEquals(
            "Merge pull request?" to "Merge #2 into main using merge.",
            pullRequestMergeConfirmation("merge", detail(), "merge", null, null),
        )
    }

    /* ── Agent tasks ─────────────────────────────────────────────────── */

    @Test
    fun `agent prompts replace the previous task's prompt and keep the user's text`() {
        val explain = pullRequestAgentPrompt(PullRequestAgentTask.Explain)
        val feedback = pullRequestAgentPrompt(PullRequestAgentTask.Feedback)
        assertEquals(explain, appendPullRequestAgentPrompt("", PullRequestAgentTask.Explain))
        assertEquals("Hi\n\n$explain", appendPullRequestAgentPrompt("Hi ", PullRequestAgentTask.Explain))
        assertEquals(feedback, appendPullRequestAgentPrompt(explain, PullRequestAgentTask.Feedback, explain))
        assertEquals(
            "Hi\n\n$feedback",
            appendPullRequestAgentPrompt("Hi\n\n$explain", PullRequestAgentTask.Feedback, explain),
        )
        // Asking adds nothing, but still drops the prompt it replaces.
        assertEquals("Hi", appendPullRequestAgentPrompt("Hi\n\n$explain", PullRequestAgentTask.Ask, explain))
        // Running the same task twice does not repeat it.
        assertEquals(explain, appendPullRequestAgentPrompt(explain, PullRequestAgentTask.Explain))
    }

    @Test
    fun `agent actions follow the PR's state`() {
        fun disabled(pr: PullRequestDetailDto, canWatch: Boolean = true) =
            pullRequestAgentActions(pr, canWatch).filter { it.disabled }.map { it.task }
        assertEquals(listOf(PullRequestAgentTask.Conflicts), disabled(detail()))
        assertEquals(emptyList<PullRequestAgentTask>(), disabled(detail(mergeability = "conflicting")))
        assertEquals(
            listOf(PullRequestAgentTask.Feedback, PullRequestAgentTask.Conflicts, PullRequestAgentTask.Watch),
            disabled(detail(state = "merged")),
        )
        assertEquals(listOf(PullRequestAgentTask.Conflicts, PullRequestAgentTask.Watch), disabled(detail(), canWatch = false))
    }

    @Test
    fun `primary label prefers a pending review, then an existing thread`() {
        assertEquals("Submit review · 2", pullRequestPrimaryActionLabel(reviewing = true, pendingComments = 2, threads = 1))
        assertEquals("Continue thread", pullRequestPrimaryActionLabel(reviewing = false, pendingComments = 2, threads = 1))
        assertEquals("Work on PR", pullRequestPrimaryActionLabel(reviewing = true, pendingComments = 0, threads = 0))
    }
}
