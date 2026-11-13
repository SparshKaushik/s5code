package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Label
import androidx.compose.material.icons.rounded.AddReaction
import androidx.compose.material.icons.rounded.Edit
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.PULL_REQUEST_REACTION_ORDER
import club.touchtech.s5code.kotlin.data.PullRequestGateway
import club.touchtech.s5code.kotlin.data.PullRequestPermissions
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.applyPendingPullRequestReactions
import club.touchtech.s5code.kotlin.data.changeRequestTerms
import club.touchtech.s5code.kotlin.data.pullRequestChecksRefreshIntervalMs
import club.touchtech.s5code.kotlin.data.pullRequestLabelColor
import club.touchtech.s5code.kotlin.data.pullRequestReactionEmoji
import club.touchtech.s5code.kotlin.data.pullRequestReactionName
import club.touchtech.s5code.kotlin.data.pullRequestReactionTooltip
import club.touchtech.s5code.kotlin.data.pullRequestReactionsSignature
import club.touchtech.s5code.kotlin.data.resolvePullRequestDetailChecks
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestChecksDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLabelCandidateDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLabelCandidateListDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestLabelDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReactionDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/* ── Live checks ─────────────────────────────────────────────────────── */

/**
 * `usePullRequestLiveChecks`: rereads only the checks while an open PR is on
 * screen, so a running CI shows up without a full detail read every poll.
 */
@Composable
fun rememberPullRequestLiveChecks(
    gateway: PullRequestGateway,
    environmentId: String,
    reference: PullRequestReference,
    detail: PullRequestDetailDto?,
    supported: Boolean,
    connected: Boolean,
): PullRequestDetailDto? {
    val read = supported && detail != null
    val checks by remember(environmentId, reference, read) {
            if (read) gateway.checks(environmentId, reference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestChecksDto?>())
    // Both reads carry the server's observedAt; without it the poll is the fresher stream.
    val live = resolvePullRequestDetailChecks(detail, checks.value, checksAreNewer = true)
    // A failed read keeps polling; only a host that answers "no checks" (null) stops it.
    val answeredNone = checks.hasValue && checks.error == null && checks.value == null
    val enabled = connected && supported && live?.state == "open" && !answeredNone
    val intervalMs = pullRequestChecksRefreshIntervalMs(live?.checks.orEmpty())
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(enabled, intervalMs, environmentId, reference) {
        if (!enabled) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                delay(intervalMs)
                try {
                    gateway.refreshChecks(environmentId, reference)
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    // The entry keeps the error beside the last value; the next tick retries.
                }
            }
        }
    }
    return live
}

/* ── Reactions ───────────────────────────────────────────────────────── */

private val PILL_SHAPE = CircleShape

/**
 * `ReactionPills`: the host's reactions with taps not yet confirmed drawn over
 * them. The overlay is keyed by what the host last reported, so the refetch
 * that lands a change drops it.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PullRequestReactionPills(
    reactions: List<PullRequestReactionDto>,
    canReact: Boolean,
    onToggle: suspend (content: String, reacted: Boolean) -> Unit,
    onFailure: () -> Unit,
    modifier: Modifier = Modifier,
    trailing: @Composable () -> Unit = {},
) {
    val scope = rememberCoroutineScope()
    val signature = pullRequestReactionsSignature(reactions)
    var pending by remember { mutableStateOf("" to emptyMap<String, Boolean>()) }
    val values = if (pending.first == signature) pending.second else emptyMap()
    val shown = applyPendingPullRequestReactions(reactions, values)
    var menuOpen by remember { mutableStateOf(false) }
    fun toggle(content: String, reacted: Boolean) {
        pending = signature to (values + (content to reacted))
        scope.launch {
            try {
                onToggle(content, reacted)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                pending = pending.first to (pending.second - content)
                onFailure()
            }
        }
    }
    FlowRow(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        shown.forEach { reaction ->
            val tooltip = pullRequestReactionTooltip(reaction)
            Row(
                Modifier.heightIn(min = 32.dp)
                    .clip(PILL_SHAPE)
                    .border(
                        1.dp,
                        if (reaction.viewerHasReacted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant,
                        PILL_SHAPE,
                    )
                    .background(
                        if (reaction.viewerHasReacted) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainer
                    )
                    .clickable(enabled = canReact) { toggle(reaction.content, !reaction.viewerHasReacted) }
                    .semantics {
                        contentDescription = tooltip
                        selected = reaction.viewerHasReacted
                    }
                    .padding(horizontal = S5Theme.spacing.medium),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                Text(pullRequestReactionEmoji(reaction.content), style = MaterialTheme.typography.bodyMedium)
                Text(
                    reaction.count.toString(),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (canReact) {
            S5OverflowMenu(
                icon = Icons.Rounded.AddReaction,
                label = "Add a reaction",
                options =
                    PULL_REQUEST_REACTION_ORDER.map { content ->
                        S5MenuOption(
                            id = content,
                            label = "${pullRequestReactionEmoji(content)}  ${pullRequestReactionName(content)}",
                            selected = shown.any { it.content == content && it.viewerHasReacted },
                        )
                    },
                onSelect = { content -> toggle(content, !shown.any { it.content == content && it.viewerHasReacted }) },
                expanded = menuOpen,
                onExpandedChange = { menuOpen = it },
            )
        }
        trailing()
    }
}

/**
 * `PullRequestCommentFooter`: reactions and an Edit button under one comment.
 * Review summaries cannot be rewritten through comment mutations, so a
 * `review` never offers Edit.
 */
@Composable
fun PullRequestCommentFooter(
    commentId: String,
    kind: String,
    authorLogin: String?,
    canEdit: Boolean?,
    reactions: List<PullRequestReactionDto>,
    actions: PullRequestCommentActions,
    editable: Boolean = true,
) {
    val canReact = actions.permissions.canReact
    val showEdit = editable && kind != "review" && actions.permissions.canEditComment(kind, authorLogin, canEdit)
    if (reactions.isEmpty() && !canReact && !showEdit) return
    PullRequestReactionPills(
        reactions = reactions,
        canReact = canReact,
        onToggle = { content, reacted -> actions.onReact(commentId, content, reacted) },
        onFailure = { actions.onError("Couldn't save the reaction. Try again in a moment.") },
    ) {
        if (showEdit) {
            S5Button(
                text = "Edit",
                icon = Icons.Rounded.Edit,
                style = S5ButtonStyle.Text,
                emphasis = S5ActionEmphasis.Secondary,
                onClick = { actions.onEdit(commentId, kind) },
            )
        }
    }
}

/** What a comment footer needs from its screen. */
class PullRequestCommentActions(
    val permissions: PullRequestPermissions,
    /** Reacts to the comment [subjectId] names, or to the PR itself when null. */
    val onReact: suspend (subjectId: String?, content: String, reacted: Boolean) -> Unit,
    val onEdit: (commentId: String, kind: String) -> Unit,
    val onError: (String) -> Unit,
)

/* ── Labels row ──────────────────────────────────────────────────────── */

/** The overview's labels, with "Edit" or "Add labels" when the viewer may change them. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PullRequestLabelsRow(labels: List<PullRequestLabelDto>, editable: Boolean, onEdit: () -> Unit) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        labels.forEach { label ->
            Row(
                Modifier.clip(MaterialTheme.shapes.small)
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                    .padding(horizontal = S5Theme.spacing.small, vertical = S5Theme.spacing.tiny),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                LabelDot(label.color)
                Text(label.name, style = MaterialTheme.typography.labelMedium)
            }
        }
        if (editable) {
            S5Button(
                text = if (labels.isEmpty()) "Add labels" else "Edit",
                icon = Icons.AutoMirrored.Rounded.Label,
                style = S5ButtonStyle.Text,
                emphasis = S5ActionEmphasis.Secondary,
                onClick = onEdit,
            )
        }
    }
}

@Composable
private fun LabelDot(color: String?) {
    Box(
        Modifier.size(8.dp)
            .clip(CircleShape)
            .background(pullRequestLabelColor(color)?.let { Color(it) } ?: MaterialTheme.colorScheme.outline)
    )
}

@Composable
private fun Muted(message: String, modifier: Modifier = Modifier) {
    Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier)
}

/* ── Edit screen ─────────────────────────────────────────────────────── */

/** A comment the edit screen found in the cached activity. */
private data class EditableComment(val body: String, val kind: String, val authorLogin: String?, val canEdit: Boolean?)

private fun findEditableComment(activity: PullRequestActivityDto?, commentId: String): EditableComment? {
    activity ?: return null
    activity.comments.firstOrNull { it.id == commentId }?.let {
        return EditableComment(it.body, it.kind, it.author?.login, it.canEdit)
    }
    return activity.reviewThreads.firstNotNullOfOrNull { thread ->
        thread.comments.firstOrNull { it.id == commentId }
    }?.let { EditableComment(it.body, "review-comment", it.author?.login, it.canEdit) }
}

private const val TITLE_MAX_CHARS = 1024
private const val EDIT_BODY_MAX_CHARS = 65_536

/**
 * `PullRequestEditScreen`: rewrite the PR's title or description, or one of
 * the viewer's comments ([field]: title | body | comment). Comments are read
 * from the cached activity by [commentId] rather than carried in the route.
 */
@Composable
fun PullRequestEditScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    field: String,
    commentId: String?,
    onBack: () -> Unit,
) {
    val gateway = store.pullRequests
    val scope = rememberCoroutineScope()
    val detailQuery = rememberPullRequestDetail(store, environmentId, reference)
    val pr = detailQuery.value
    val permissions = rememberPullRequestPermissions(store, environmentId, pr)
    val activityQuery by remember(environmentId, reference, field) {
            if (field == "comment") gateway.activity(environmentId, reference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestActivityDto>())
    val comment = commentId?.let { findEditableComment(activityQuery.value, it) }
    val terms = changeRequestTerms(pr?.provider)
    val original =
        when (field) {
            "title" -> pr?.title
            "body" -> pr?.body
            else -> comment?.body
        }
    var draft by rememberSaveable { mutableStateOf<String?>(null) }
    val value = draft ?: original.orEmpty()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val allowed =
        if (field == "comment") {
            comment != null && permissions.canEditComment(comment.kind, comment.authorLogin, comment.canEdit)
        } else {
            permissions.canEditChangeRequest
        }
    // Name the first thing in the way, so the notice says what would actually fix it.
    val readOnlyReason =
        when {
            pr == null || allowed -> null
            !permissions.connected -> "Reconnect to save changes."
            !permissions.canWrite -> "This device isn't allowed to change source control on this environment."
            field == "comment" && comment == null && activityQuery.hasValue -> "This comment isn't loaded anymore."
            field == "comment" -> "You can't edit this comment."
            else -> "You can't edit this ${terms.singular}."
        }
    val unchanged = draft == null || draft == original
    val invalid = field == "title" && value.isBlank()

    fun save() {
        if (!allowed || unchanged || invalid || busy) return
        val submitted = value
        busy = true
        error = null
        scope.launch {
            try {
                when (field) {
                    "title" -> gateway.update(environmentId, reference, title = submitted.trim())
                    "body" -> gateway.update(environmentId, reference, body = submitted)
                    else -> gateway.updateComment(environmentId, reference, checkNotNull(commentId), checkNotNull(comment).kind, submitted)
                }
                onBack()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                error = failure.message ?: "The provider refused the change."
            } finally {
                busy = false
            }
        }
    }

    S5Screen(
        title =
            when (field) {
                "title" -> "Edit title"
                "body" -> "Edit description"
                else -> "Edit comment"
            },
        subtitle = "#${reference.number} · ${reference.repository}",
        onBack = onBack,
        loading = busy,
    ) { padding ->
        when {
            pr == null && detailQuery.error != null ->
                S5EmptyState(
                    icon = Icons.Rounded.Edit,
                    title = "Couldn't load this ${terms.singular}",
                    detail = detailQuery.error.orEmpty(),
                    modifier = Modifier.padding(padding),
                )
            original == null && (pr == null || (field == "comment" && !activityQuery.hasValue)) ->
                S5LoadingState("Loading", Modifier.padding(padding))
            else ->
                Column(
                    Modifier.fillMaxSize()
                        .padding(padding)
                        .imePadding()
                        .verticalScroll(rememberScrollState())
                        .padding(S5Theme.spacing.gutter),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                ) {
                    S5TextField(
                        value = value,
                        onValueChange = { draft = it.take(if (field == "title") TITLE_MAX_CHARS else EDIT_BODY_MAX_CHARS) },
                        label =
                            when (field) {
                                "title" -> "${terms.title} title"
                                "body" -> "${terms.title} description"
                                else -> "Comment"
                            },
                        placeholder = if (field == "title") "Title" else "Write in Markdown…",
                        enabled = !busy && original != null,
                        singleLine = field == "title",
                        maxLines = if (field == "title") 1 else 18,
                        minHeight = if (field == "title") 56.dp else 192.dp,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    readOnlyReason?.let { Muted(it) }
                    (error ?: detailQuery.error ?: activityQuery.error)?.let {
                        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                    }
                    S5Button(
                        text = if (busy) "Saving…" else "Save",
                        enabled = allowed && !unchanged && !invalid && !busy && original != null,
                        onClick = ::save,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
        }
    }
}

/* ── Labels screen ───────────────────────────────────────────────────── */

/**
 * `PullRequestLabelsScreen`: add or remove labels. Rows keep the host's order;
 * re-sorting on toggle would move a row out from under a tap.
 */
@Composable
fun PullRequestLabelsScreen(store: AppStore, environmentId: String, reference: PullRequestReference, onBack: () -> Unit) {
    val gateway = store.pullRequests
    val scope = rememberCoroutineScope()
    val detailQuery = rememberPullRequestDetail(store, environmentId, reference)
    val pr = detailQuery.value
    val permissions = rememberPullRequestPermissions(store, environmentId, pr)
    val allowed = permissions.canManageLabels
    val supported = pr?.capabilities?.labels == true
    val result by remember(environmentId, reference, supported) {
            if (supported) gateway.labelCandidates(environmentId, reference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestLabelCandidateListDto>())
    val readOnlyReason =
        when {
            pr == null || allowed -> null
            !permissions.connected -> "Reconnect to change labels."
            !permissions.canWrite -> "This device isn't allowed to change source control on this environment."
            !supported -> "This provider doesn't support changing labels."
            else -> "Changing labels needs triage access on this repository."
        }
    var query by rememberSaveable { mutableStateOf("") }
    var pending by remember { mutableStateOf<Pair<String, Boolean>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val needle = query.trim().lowercase()
    val candidates =
        remember(result.value, needle) {
            result.value?.candidates.orEmpty().filter { "${it.name} ${it.description.orEmpty()}".lowercase().contains(needle) }
        }

    fun toggle(candidate: PullRequestLabelCandidateDto) {
        if (!allowed || pending != null) return
        val applied = !candidate.isApplied
        pending = candidate.name to applied
        error = null
        scope.launch {
            try {
                gateway.setLabels(environmentId, reference, listOf(candidate.name), applied)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                error = failure.message ?: "Couldn't update this label. Try again."
            } finally {
                pending = null
            }
        }
    }

    S5Screen(
        title = "Labels",
        subtitle = "#${reference.number} · ${reference.repository}",
        onBack = onBack,
        loading = result.loading && result.hasValue,
    ) { padding ->
        when {
            pr == null && detailQuery.error != null ->
                S5EmptyState(
                    icon = Icons.AutoMirrored.Rounded.Label,
                    title = "Couldn't load this ${changeRequestTerms(null).singular}",
                    detail = detailQuery.error.orEmpty(),
                    modifier = Modifier.padding(padding),
                )
            pr == null -> S5LoadingState("Loading labels", Modifier.padding(padding))
            else ->
                Column(Modifier.fillMaxSize().padding(padding).imePadding()) {
                    Column(
                        Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.medium),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        S5SearchField(value = query, onValueChange = { query = it }, placeholder = "Search labels")
                        readOnlyReason?.let { Muted(it) }
                        error?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error) }
                    }
                    Box(Modifier.weight(1f)) {
                        when {
                            candidates.isNotEmpty() ->
                                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = S5Theme.spacing.xxLarge)) {
                                    items(candidates, key = { it.name }) { candidate ->
                                        LabelCandidateRow(
                                            candidate = candidate,
                                            // The change shows as soon as it is tapped; a failure puts the host's answer back.
                                            checked = pending?.takeIf { it.first == candidate.name }?.second ?: candidate.isApplied,
                                            pending = pending?.first == candidate.name,
                                            enabled = allowed && pending == null,
                                            onClick = { toggle(candidate) },
                                        )
                                    }
                                    if (result.value?.truncated == true) {
                                        item(key = "truncated") {
                                            Muted(
                                                "This repository has more labels than are listed here. Apply the rest on the provider.",
                                                Modifier.padding(S5Theme.spacing.gutter),
                                            )
                                        }
                                    }
                                }
                            !result.hasValue && result.error == null && supported -> S5LoadingState("Loading labels")
                            else ->
                                S5EmptyState(
                                    icon = Icons.AutoMirrored.Rounded.Label,
                                    title =
                                        when {
                                            result.error != null -> "Couldn't load labels"
                                            needle.isNotEmpty() -> "No matching labels"
                                            else -> "This repository has no labels"
                                        },
                                    detail =
                                        result.error
                                            ?: if (needle.isNotEmpty()) "Try another name." else "Create labels on the provider to use them here.",
                                    actionLabel = if (result.error != null) "Retry" else null,
                                    onAction = { scope.launch { gateway.refreshLabelCandidates(environmentId, reference) } },
                                )
                        }
                    }
                }
        }
    }
}

@Composable
private fun LabelCandidateRow(
    candidate: PullRequestLabelCandidateDto,
    checked: Boolean,
    pending: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Column {
        Row(
            Modifier.fillMaxWidth()
                .heightIn(min = 56.dp)
                .clickable(enabled = enabled, onClickLabel = if (checked) "Remove label" else "Add label", onClick = onClick)
                .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            LabelDot(candidate.color)
            Column(Modifier.weight(1f)) {
                Text(candidate.name, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                candidate.description?.takeIf(String::isNotBlank)?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            if (pending) S5InlineLoading() else Checkbox(checked = checked, onCheckedChange = null, enabled = enabled)
        }
        HorizontalDivider(Modifier.padding(start = S5Theme.spacing.gutter), color = MaterialTheme.colorScheme.outlineVariant)
    }
}
