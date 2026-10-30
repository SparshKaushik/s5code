package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Undo
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.data.PULL_REQUEST_REVIEW_VERDICTS
import club.touchtech.s5code.kotlin.data.PendingReviewComment
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.PullRequestReviewPosition
import club.touchtech.s5code.kotlin.data.ReviewDraft
import club.touchtech.s5code.kotlin.data.canSubmitPullRequestReview
import club.touchtech.s5code.kotlin.data.clearSubmittedReview
import club.touchtech.s5code.kotlin.data.mergeDiscussionComments
import club.touchtech.s5code.kotlin.data.pullRequestReviewKey
import club.touchtech.s5code.kotlin.data.relativeLabel
import club.touchtech.s5code.kotlin.data.reviewSummaryRequired
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestThreadCommentDto
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/* Discussion, inline comment and review screens, ported from RN's
 * `PullRequestDiscussionScreen`, `PullRequestInlineCommentScreen` and
 * `PullRequestReviewSheet`. */

private const val BODY_MAX_CHARS = 65_536

@Composable
private fun ErrorText(message: String) {
    Text(message, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
}

@Composable
private fun MutedText(message: String) {
    Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

/** One review thread: its comments (paged), replying, and resolving. */
@Composable
fun PullRequestDiscussionScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    discussionId: String,
    onBack: () -> Unit,
    onOpenRoute: (String) -> Unit,
) {
    val gateway = store.pullRequests
    val scope = rememberCoroutineScope()
    val activityQuery by remember(environmentId, reference) { gateway.activity(environmentId, reference) }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestActivityDto>())
    val detail = rememberPullRequestDetail(store, environmentId, reference).value
    val permissions = rememberPullRequestPermissions(store, environmentId, detail)
    val thread = activityQuery.value?.reviewThreads?.firstOrNull { it.id == discussionId }
    var body by rememberSaveable { mutableStateOf("") }
    var extra by remember { mutableStateOf<List<PullRequestThreadCommentDto>>(emptyList()) }
    // Unset until a page loads: the thread's own cursor answers before then.
    var cursor by remember { mutableStateOf<Pair<Boolean, String?>>(false to null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }

    fun operation(run: suspend () -> Unit) {
        if (busy) return
        busy = true
        error = null
        notice = null
        scope.launch {
            try {
                run()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                error = failure.message ?: "The provider could not complete this action."
            } finally {
                busy = false
            }
        }
    }

    val nextCursor = if (cursor.first) cursor.second else thread?.nextCommentsCursor
    val comments = remember(thread, extra) { mergeDiscussionComments(thread?.comments.orEmpty(), extra) }
    val canReply = permissions.canReply && thread != null
    val now = remember(comments) { System.currentTimeMillis() }
    val commentActions =
        PullRequestCommentActions(
            permissions = permissions,
            onReact = { subjectId, content, reacted -> gateway.setReaction(environmentId, reference, subjectId, content, reacted) },
            onEdit = { commentId, _ -> onOpenRoute(Routes.pullRequestEdit(environmentId, reference, "comment", commentId)) },
            onError = store::showError,
        )

    S5Screen(
        title = "Discussion",
        subtitle = "#${reference.number}",
        onBack = onBack,
        loading = activityQuery.loading && thread != null,
        actions = {
            if (permissions.canResolve && thread != null) {
                S5IconButton(
                    icon = if (thread.isResolved) Icons.AutoMirrored.Rounded.Undo else Icons.Rounded.Check,
                    label = if (thread.isResolved) "Unresolve" else "Resolve",
                    enabled = !busy && !activityQuery.loading,
                    onClick = {
                        operation { gateway.setThreadResolution(environmentId, reference, thread.id, !thread.isResolved) }
                    },
                )
            }
        },
        bottomBar = {
            if (canReply) {
                Row(
                    Modifier.fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surfaceContainer)
                        .navigationBarsPadding()
                        .imePadding()
                        .padding(S5Theme.spacing.small),
                    verticalAlignment = Alignment.Bottom,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    S5TextField(
                        value = body,
                        onValueChange = { body = it.take(BODY_MAX_CHARS) },
                        placeholder = "Reply…",
                        enabled = !busy,
                        modifier = Modifier.weight(1f),
                    )
                    S5Button(
                        text = if (busy) "Posting…" else "Reply",
                        enabled = !busy && body.isNotBlank(),
                        emphasis = S5ActionEmphasis.Secondary,
                        onClick = {
                            val current = thread ?: return@S5Button
                            val submitted = body
                            operation {
                                gateway.replyToThread(environmentId, reference, current.id, submitted)
                                if (body == submitted) body = ""
                                notice = "Reply posted."
                            }
                        },
                    )
                }
            }
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            when {
                thread != null -> {
                    Text(thread.path + (thread.line?.let { ":$it" } ?: ""), style = S5Theme.code.code)
                    MutedText((if (thread.isResolved) "Resolved" else "Unresolved") + if (thread.isOutdated) " · Outdated" else "")
                    S5Button(
                        text = "View file diff",
                        style = S5ButtonStyle.Tonal,
                        emphasis = S5ActionEmphasis.Secondary,
                        onClick = { onOpenRoute(Routes.pullRequestDiff(environmentId, reference, path = thread.path)) },
                    )
                }
                activityQuery.loading || (!activityQuery.hasValue && activityQuery.error == null) ->
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { S5InlineLoading() }
                else -> Text("This discussion is no longer available.")
            }
            comments.forEach { comment ->
                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                        PullRequestAvatar(comment.author, size = 28.dp)
                        Text(
                            comment.author?.login ?: "Unknown author",
                            style = MaterialTheme.typography.titleSmall,
                            modifier = Modifier.weight(1f),
                        )
                        MutedText(relativeLabel(comment.createdAt, now))
                    }
                    S5Markdown(source = comment.body, html = true)
                    PullRequestCommentFooter(
                        commentId = comment.id,
                        kind = "review-comment",
                        authorLogin = comment.author?.login,
                        canEdit = comment.canEdit,
                        reactions = comment.reactions.orEmpty(),
                        actions = commentActions,
                        // The edit screen reads bodies from the cached activity; a reply
                        // loaded page by page is not in it.
                        editable = thread?.comments?.any { it.id == comment.id } == true,
                    )
                }
            }
            if (nextCursor != null) {
                S5Button(
                    text = if (busy) "Loading…" else "Load more replies",
                    enabled = !busy,
                    style = S5ButtonStyle.Text,
                    onClick = {
                        val current = thread ?: return@S5Button
                        operation {
                            val page = gateway.threadComments(environmentId, reference, current.id, nextCursor)
                            extra = extra + page.comments
                            cursor = true to page.nextCursor
                        }
                    },
                )
            }
            (error ?: activityQuery.error)?.let { ErrorText(it) }
            notice?.let { MutedText(it) }
        }
    }
}

/**
 * A pending line comment: written here, saved to this device's review draft
 * on the revision it was written against, sent with the next review.
 */
@Composable
fun PullRequestInlineCommentScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    draftId: String?,
    path: String,
    oldPath: String?,
    position: PullRequestReviewPosition,
    headSha: String,
    code: String,
    confirmController: S5ConfirmDialogController,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val drafts = store.reviewDrafts
    val ready by drafts.ready.collectAsStateWithLifecycle()
    val all by drafts.drafts.collectAsStateWithLifecycle()
    val key = pullRequestReviewKey(environmentId, reference)
    val saved = all[key]?.comments?.firstOrNull { it.id == draftId }
    var edited by rememberSaveable { mutableStateOf<String?>(null) }
    val body = edited ?: saved?.body.orEmpty()
    val detailQuery = rememberPullRequestDetail(store, environmentId, reference)
    LaunchedEffect(environmentId, reference) { runCatching { store.pullRequests.refreshDetail(environmentId, reference) } }
    val detail = detailQuery.value
    val permissions = rememberPullRequestPermissions(store, environmentId, detail)
    val stale = detail != null && detail.headSha != headSha
    var busy by remember { mutableStateOf(false) }
    val canSave =
        ready && body.isNotBlank() && detail != null && detailQuery.error == null && !stale && permissions.canInlineComment

    fun save() {
        if (busy || !canSave) return
        busy = true
        drafts.update(key) { draft ->
            draft.copy(
                comments =
                    draft.comments.filter { it.id != draftId } +
                        PendingReviewComment(
                            id = draftId ?: UUID.randomUUID().toString(),
                            path = path,
                            oldPath = oldPath,
                            position = position,
                            body = body,
                            headSha = headSha,
                            code = code,
                        ),
            )
        }
        scope.launch {
            try {
                drafts.flush()
                onBack()
            } finally {
                busy = false
            }
        }
    }

    S5Screen(
        title = if (saved != null) "Edit review comment" else "Review comment",
        subtitle = "#${reference.number} · ${position.label}",
        onBack = onBack,
        bottomBar = {
            Box(Modifier.fillMaxWidth().navigationBarsPadding().imePadding().padding(S5Theme.spacing.gutter)) {
                S5Button(
                    text = if (busy) "Saving…" else "Save draft",
                    enabled = canSave && !busy,
                    onClick = ::save,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            S5Card(tone = S5CardTone.Receded) {
                Column(Modifier.padding(S5Theme.spacing.large), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                    Text(path, style = S5Theme.code.codeSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text(code, style = S5Theme.code.codeSmall)
                }
            }
            S5TextField(
                value = body,
                onValueChange = { edited = it.take(BODY_MAX_CHARS) },
                placeholder = "Write feedback on this line…",
                enabled = ready && !busy,
                maxLines = 12,
                minHeight = 128.dp,
            )
            MutedText("Saved comments are private until you submit your review.")
            if (stale) ErrorText("The branch changed. Return to the current diff and select the line again.")
            detailQuery.error?.let { ErrorText(it) }
            if (saved != null) {
                S5Button(
                    text = "Delete draft",
                    style = S5ButtonStyle.Text,
                    onClick = {
                        confirmController.show(
                            S5ConfirmDialogRequest(
                                title = "Delete this draft?",
                                message = "Your other review comments will be kept.",
                                confirmText = "Delete",
                                destructive = true,
                                onConfirm = {
                                    drafts.update(key) { draft -> draft.copy(comments = draft.comments.filter { it.id != saved.id }) }
                                    onBack()
                                },
                            )
                        )
                    },
                )
            }
        }
    }
}

/**
 * Submit review ([mode] "review": verdict, summary and pending line comments)
 * or Add comment ([mode] "comment"). The text is part of the device-local
 * draft, so leaving keeps it.
 */
@Composable
fun PullRequestReviewScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    mode: String,
    onBack: () -> Unit,
    onReplaceRoute: (String) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val drafts = store.reviewDrafts
    val ready by drafts.ready.collectAsStateWithLifecycle()
    val all by drafts.drafts.collectAsStateWithLifecycle()
    val key = pullRequestReviewKey(environmentId, reference)
    val draft = all[key] ?: ReviewDraft()
    val reviewing = mode == "review"
    val body = if (reviewing) draft.summary else draft.comment
    val detailQuery = rememberPullRequestDetail(store, environmentId, reference)
    LaunchedEffect(environmentId, reference) { runCatching { store.pullRequests.refreshDetail(environmentId, reference) } }
    val detail = detailQuery.value
    val permissions = rememberPullRequestPermissions(store, environmentId, detail)
    val offered = PULL_REQUEST_REVIEW_VERDICTS.filter { it.first in permissions.verdicts }
    var requested by rememberSaveable { mutableStateOf("comment") }
    val verdict = offered.firstOrNull { it.first == requested } ?: offered.firstOrNull()
    val headSha = detail?.headSha
    val stale = draft.isStale(headSha)
    val canSubmitComments = draft.comments.isEmpty() || permissions.canInlineComment
    val summaryRequired = reviewSummaryRequired(detail?.provider, verdict?.first)
    val canSubmit =
        ready && canSubmitPullRequestReview(reviewing, draft, body, verdict?.first, headSha, detail?.provider, permissions)
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    fun setBody(text: String) =
        drafts.update(key) { if (reviewing) it.copy(summary = text.take(BODY_MAX_CHARS)) else it.copy(comment = text.take(BODY_MAX_CHARS)) }

    fun submit() {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                if (!canSubmit || detail == null) {
                    throw IllegalStateException("Add your feedback and check your permissions before submitting.")
                }
                val submitted = draft
                if (reviewing && verdict != null) {
                    store.pullRequests.submitReview(environmentId, reference, verdict.first, body, submitted.comments)
                } else {
                    store.pullRequests.comment(environmentId, reference, body)
                }
                drafts.update(key) { current ->
                    if (reviewing) {
                        clearSubmittedReview(current, submitted)
                    } else {
                        current.copy(comment = if (current.comment == submitted.comment) "" else current.comment)
                    }
                }
                drafts.flush()
                onBack()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                error = failure.message ?: "The provider could not complete this action. Try again."
            } finally {
                busy = false
            }
        }
    }

    S5Screen(
        title = if (reviewing) "Submit review" else "Add comment",
        subtitle =
            "#${reference.number}" +
                if (reviewing && headSha != null) " · reviewing ${headSha.take(7)}" else " · ${reference.repository}",
        onBack = onBack,
        bottomBar = {
            Box(Modifier.fillMaxWidth().navigationBarsPadding().imePadding().padding(S5Theme.spacing.gutter)) {
                S5Button(
                    text =
                        when {
                            busy -> "Submitting…"
                            !reviewing -> "Post comment"
                            verdict?.first == "approve" -> "Approve"
                            verdict?.first == "request-changes" -> "Request changes"
                            else -> "Submit review"
                        },
                    enabled = canSubmit && !busy && detail != null && detailQuery.error == null,
                    onClick = ::submit,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(S5Theme.spacing.gutter),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            if (reviewing && offered.size > 1) {
                S5ConnectedButtonGroup(
                    options = offered,
                    selected = verdict ?: offered.first(),
                    onSelect = { if (!busy) requested = it.first },
                    label = { it.second },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            S5TextField(
                value = body,
                onValueChange = ::setBody,
                label = if (reviewing) "Summary" else "Comment",
                placeholder =
                    when {
                        !reviewing -> "Write a comment…"
                        summaryRequired -> "Summary required to request changes…"
                        else -> "Summarize your review…"
                    },
                enabled = ready && !busy,
                maxLines = 12,
                minHeight = 144.dp,
            )
            if (reviewing && draft.comments.isNotEmpty()) {
                Text(
                    "${draft.comments.size} pending line ${if (draft.comments.size == 1) "comment" else "comments"}",
                    style = MaterialTheme.typography.titleSmall,
                )
                draft.comments.forEach { comment ->
                    val outdated = comment.headSha != headSha
                    S5Card(tone = S5CardTone.Receded) {
                        Column(
                            Modifier.fillMaxWidth()
                                .clickable(enabled = !busy) {
                                    onReplaceRoute(
                                        if (outdated) {
                                            Routes.pullRequestDiff(environmentId, reference, path = comment.path, replacingDraftId = comment.id)
                                        } else {
                                            Routes.pullRequestInlineComment(
                                                environmentId,
                                                reference,
                                                path = comment.path,
                                                oldPath = comment.oldPath,
                                                position = comment.position,
                                                headSha = comment.headSha,
                                                code = comment.code,
                                                draftId = comment.id,
                                            )
                                        }
                                    )
                                }
                                .padding(S5Theme.spacing.large),
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                        ) {
                            Text(
                                "${comment.path} · ${comment.position.label}",
                                style = S5Theme.code.codeSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Text(comment.body, style = MaterialTheme.typography.bodyMedium, maxLines = 3)
                            MutedText(if (outdated) "Branch changed · Tap to choose a new line" else "Tap to edit")
                        }
                    }
                }
            }
            if (reviewing && stale) {
                ErrorText("The branch changed since you wrote a comment. Choose its current line before submitting.")
            }
            if (reviewing && !canSubmitComments) {
                ErrorText("You can no longer submit line comments on this pull request. Your drafts are kept here.")
            }
            (error ?: detailQuery.error)?.let { ErrorText(it) }
        }
    }
}
