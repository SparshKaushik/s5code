package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallSplit
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.Cancel
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.ExpandLess
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Group
import androidx.compose.material.icons.rounded.Layers
import androidx.compose.material.icons.rounded.RemoveCircleOutline
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.Error
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.PullRequestActivityFilter
import club.touchtech.s5code.kotlin.data.PullRequestProjectIdentity
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.PullRequestTone
import club.touchtech.s5code.kotlin.data.checkStatus
import club.touchtech.s5code.kotlin.data.openOnProviderLabel
import club.touchtech.s5code.kotlin.data.pullRequestAttachments
import club.touchtech.s5code.kotlin.data.pullRequestBranchSummary
import club.touchtech.s5code.kotlin.data.pullRequestChecksSummary
import club.touchtech.s5code.kotlin.data.pullRequestChecksUrl
import club.touchtech.s5code.kotlin.data.pullRequestConflict
import club.touchtech.s5code.kotlin.data.pullRequestFallbackEnvironment
import club.touchtech.s5code.kotlin.data.pullRequestLabelColor
import club.touchtech.s5code.kotlin.data.pullRequestReviewSummary
import club.touchtech.s5code.kotlin.data.pullRequestStatus
import club.touchtech.s5code.kotlin.data.relativeLabel
import club.touchtech.s5code.kotlin.data.sortedPullRequestChecks
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5ConnectedButtonGroup
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ErrorState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5ImageLightbox
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5PullToRefreshBox
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActivityDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestActorDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestDetailDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListEntryDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestStackDto
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/** The PR page's segments. Guide (phase 3) and Files (phase 2) slot in here. */
private enum class PullRequestSegment(val label: String) {
    Overview("Overview"),
    Activity("Activity"),
}

private const val DESCRIPTION_COLLAPSED_HEIGHT = 240

/** What the identity block draws, from the detail or, before it loads, the listing row. */
private data class PullRequestIdentity(
    val title: String,
    val state: String,
    val isDraft: Boolean,
    val mergeability: String,
    val additions: Int,
    val deletions: Int,
    val changedFiles: Int?,
    val author: PullRequestActorDto?,
    val updatedAt: String,
    val baseBranch: String,
    val headBranch: String,
    val url: String,
    val provider: String,
)

private fun PullRequestDetailDto.identity() =
    PullRequestIdentity(title, state, isDraft, mergeability, additions, deletions, changedFiles, author, updatedAt, baseBranch, headBranch, url, provider)

private fun PullRequestListEntryDto.identity() =
    PullRequestIdentity(
        title, state, isDraft, mergeability, additions, deletions, null, author, updatedAt, baseBranch, headBranch, url, provider,
    )

/**
 * A read-only PR page: identity, then Overview (stack, checks, reviewers,
 * branch, labels, description) or Activity (the filtered timeline). Reads come
 * from the PR's own environment, or a connected one with the same repository
 * while it is offline, and fall back to the last copy saved on this device.
 */
@Composable
fun PullRequestDetailScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    onBack: () -> Unit,
    onOpenPullRequest: (environmentId: String, reference: PullRequestReference) -> Unit,
) {
    val gateway = store.pullRequests
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val connectedIds =
        remember(environments) { environments.filter { it.state == ConnectionState.Connected }.mapTo(HashSet()) { it.id.value } }
    val fallback =
        remember(environmentId, reference, projects, connectedIds) {
            pullRequestFallbackEnvironment(
                environmentId = environmentId,
                projectId = reference.projectId,
                repository = reference.repository,
                host = reference.host,
                projects =
                    projects.map {
                        PullRequestProjectIdentity(it.environmentId.value, it.id.value, it.repositoryIdentity?.canonicalKey)
                    },
                connectedEnvironmentIds = connectedIds,
            )
        }
    val readEnvironment = fallback?.environmentId ?: environmentId
    val readReference = fallback?.let { reference.copy(projectId = it.projectId) } ?: reference
    val environment = environments.firstOrNull { it.id.value == readEnvironment }
    val online by remember(readEnvironment) { gateway.connected(readEnvironment) }.collectAsStateWithLifecycle(false)

    val detailQuery by remember(readEnvironment, readReference) { gateway.detail(readEnvironment, readReference) }
        .collectAsStateWithLifecycle(PullRequestQuery())
    val activityQuery by remember(readEnvironment, readReference) { gateway.activity(readEnvironment, readReference) }
        .collectAsStateWithLifecycle(PullRequestQuery())
    val detail = detailQuery.value
    val stackSupported = environment?.capabilities?.threadPullRequests == true && detail?.capabilities?.stacks == true
    val stackQuery by remember(readEnvironment, readReference, stackSupported) {
            if (stackSupported) gateway.stack(readEnvironment, readReference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery())
    val summary = remember(readEnvironment, readReference, detail) { gateway.summary(readEnvironment, readReference) }
    val identity = detail?.identity() ?: summary?.identity()

    var segment by rememberSaveable { mutableStateOf(PullRequestSegment.Overview) }
    var activityFilter by remember { mutableStateOf(PullRequestActivityFilter()) }
    var refreshing by remember { mutableStateOf(false) }
    fun refresh() {
        if (refreshing) return
        refreshing = true
        scope.launch {
            try {
                gateway.refresh(readEnvironment, readReference)
            } finally {
                refreshing = false
            }
        }
    }

    S5Screen(
        title = "#${reference.number}" + if (online) "" else " · Offline",
        subtitle = reference.repository,
        onBack = onBack,
        loading = refreshing || (detailQuery.loading && detail != null),
        actions = {
            identity?.url?.takeIf { it.isNotEmpty() }?.let { url ->
                S5IconButton(
                    icon = Icons.AutoMirrored.Rounded.OpenInNew,
                    label = openOnProviderLabel(identity.provider),
                    onClick = {
                        if (!openPullRequestUrl(context, url)) store.showError("The pull request could not be opened.")
                    },
                )
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when {
                identity == null && detailQuery.error != null ->
                    S5ErrorState(
                        title = "Couldn't load this pull request",
                        detail = detailQuery.error.orEmpty(),
                        onRetry = ::refresh,
                        modifier = Modifier.padding(S5Theme.spacing.gutter),
                    )
                identity == null && !online ->
                    S5EmptyState(
                        icon = Icons.Rounded.CloudOff,
                        title = "Environment offline",
                        detail = "Reconnect to load this pull request. It isn't saved on this device yet.",
                    )
                identity == null -> S5LoadingState("Loading pull request")
                else ->
                    S5PullToRefreshBox(isRefreshing = refreshing, onRefresh = ::refresh, modifier = Modifier.fillMaxSize()) {
                        LazyColumn(
                            modifier = Modifier.fillMaxSize(),
                            contentPadding =
                                PaddingValues(
                                    start = S5Theme.spacing.gutter,
                                    end = S5Theme.spacing.gutter,
                                    bottom = S5Theme.spacing.xxLarge,
                                ),
                            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                        ) {
                            detailQuery.error?.let { message ->
                                item(key = "error") { S5Notice(icon = Icons.Rounded.CloudOff, text = message) }
                            }
                            item(key = "identity") { PullRequestIdentityBlock(identity) }
                            item(key = "segments") {
                                S5ConnectedButtonGroup(
                                    options = PullRequestSegment.entries,
                                    selected = segment,
                                    onSelect = { segment = it },
                                    label = { it.label },
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                            when (segment) {
                                PullRequestSegment.Overview ->
                                    if (detail == null) {
                                        item(key = "overview-loading") {
                                            Box(Modifier.fillMaxWidth().padding(S5Theme.spacing.xxLarge), contentAlignment = Alignment.Center) {
                                                S5InlineLoading()
                                            }
                                        }
                                    } else {
                                        item(key = "stack") {
                                            PullRequestStackCard(
                                                number = reference.number,
                                                stack = stackQuery.value,
                                                summaryStack = summary?.stack?.takeIf { summary.number == reference.number },
                                                onOpenLayer = { number -> onOpenPullRequest(environmentId, reference.copy(number = number)) },
                                            )
                                        }
                                        item(key = "status") {
                                            PullRequestStatusCard(detail, activityQuery.value, onOpenUrl = { url ->
                                                if (!openPullRequestUrl(context, url)) store.showError("The link could not be opened.")
                                            })
                                        }
                                        if (detail.labels.isNotEmpty()) {
                                            item(key = "labels") { PullRequestLabels(detail) }
                                        }
                                        item(key = "description") { PullRequestDescription(detail.body) }
                                    }
                                PullRequestSegment.Activity ->
                                    pullRequestActivityItems(
                                        activity = activityQuery.value,
                                        filter = activityFilter,
                                        onFilterChange = { activityFilter = it },
                                        loading = !activityQuery.hasValue && (activityQuery.loading || online),
                                        error = activityQuery.error,
                                    )
                            }
                        }
                    }
            }
        }
    }
}

@Composable
private fun PullRequestIdentityBlock(identity: PullRequestIdentity) {
    val copy = rememberClipboardWriter()
    val status = pullRequestStatus(identity.state, identity.isDraft)
    val conflict = pullRequestConflict(identity.state, identity.isDraft, identity.mergeability, identity.baseBranch)
    val now = remember(identity.updatedAt) { System.currentTimeMillis() }
    Column(Modifier.padding(top = S5Theme.spacing.small), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
            Icon(status.icon, contentDescription = null, tint = status.tone.color(), modifier = Modifier.size(18.dp))
            Text(status.label, style = MaterialTheme.typography.labelLarge, color = status.tone.color())
            if (identity.additions + identity.deletions > 0) {
                Text("+${identity.additions}", style = S5Theme.code.code, color = S5Theme.status.added)
                Text("−${identity.deletions}", style = S5Theme.code.code, color = S5Theme.status.removed)
            }
            identity.changedFiles?.takeIf { it > 0 }?.let {
                Text(
                    if (it == 1) "1 file" else "$it files",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Text(identity.title, style = MaterialTheme.typography.titleLargeEmphasized)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
            PullRequestAvatar(identity.author, size = 20.dp)
            Text(
                listOfNotNull(identity.author?.login, relativeLabel(identity.updatedAt, now).takeIf { it.isNotEmpty() }?.let { "Updated $it" })
                    .joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (identity.baseBranch.isNotEmpty() || identity.headBranch.isNotEmpty()) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Icon(
                    Icons.AutoMirrored.Rounded.CallSplit,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(14.dp),
                )
                Text(
                    identity.baseBranch,
                    style = S5Theme.code.code,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false).clickable { copy(identity.baseBranch) },
                )
                Text("←", style = S5Theme.code.code, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(
                    identity.headBranch,
                    style = S5Theme.code.code,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false).clickable { copy(identity.headBranch) },
                )
            }
        }
        conflict?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = S5Theme.status.failed) }
    }
}

/** One expandable line of an overview card: icon, title, value, then optional detail. */
@Composable
private fun OverviewRow(
    icon: ImageVector,
    title: String,
    value: String?,
    tint: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    expanded: Boolean? = null,
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit = {},
) {
    Column {
        Row(
            Modifier.fillMaxWidth()
                .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
                .padding(horizontal = S5Theme.spacing.large, vertical = S5Theme.spacing.medium),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(20.dp))
            Text(title, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            value?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            }
            if (expanded != null) {
                Icon(
                    if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                    contentDescription = if (expanded) "Collapse" else "Expand",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(18.dp),
                )
            }
        }
        if (expanded != false) content()
    }
}

@Composable
private fun PullRequestStackCard(
    number: Int,
    stack: PullRequestStackDto?,
    summaryStack: club.touchtech.s5code.kotlin.transport.wire.PullRequestStackMembershipDto?,
    onOpenLayer: (Int) -> Unit,
) {
    if (stack == null || stack.layers.isEmpty()) {
        val saved = summaryStack ?: return
        S5Card(tone = club.touchtech.s5code.kotlin.design.component.S5CardTone.Receded) {
            OverviewRow(icon = Icons.Rounded.Layers, title = "Stack", value = "${saved.position} of ${saved.size}")
        }
        return
    }
    var expanded by rememberSaveable(stack.id) { mutableStateOf(false) }
    val position = stack.layers.indexOfFirst { it.number == number } + 1
    S5Card(tone = club.touchtech.s5code.kotlin.design.component.S5CardTone.Receded) {
        OverviewRow(
            icon = Icons.Rounded.Layers,
            title = "Stack",
            value = "$position of ${stack.layers.size}",
            expanded = expanded,
            onClick = { expanded = !expanded },
        ) {
            stack.layers.forEachIndexed { index, layer ->
                val selected = layer.number == number
                val status = pullRequestStatus(layer.state, layer.isDraft == true)
                Row(
                    Modifier.fillMaxWidth()
                        .then(
                            if (selected) Modifier.background(MaterialTheme.colorScheme.secondaryContainer)
                            else Modifier.clickable { onOpenLayer(layer.number) }
                        )
                        .padding(horizontal = S5Theme.spacing.large, vertical = S5Theme.spacing.small),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                ) {
                    Icon(status.icon, contentDescription = null, tint = status.tone.color(), modifier = Modifier.size(18.dp))
                    Column(Modifier.weight(1f)) {
                        Text(layer.title ?: layer.headBranch, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            "#${layer.number} · ${index + 1} of ${stack.layers.size}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Text(
                        if (selected) "Current" else status.label,
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Text(
                "Base: ${stack.base}. Open a layer to review it or merge the stack through that layer.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = S5Theme.spacing.large, vertical = S5Theme.spacing.medium),
            )
        }
    }
}

private fun toneIcon(tone: PullRequestTone): ImageVector =
    when (tone) {
        PullRequestTone.Success -> Icons.Rounded.CheckCircle
        PullRequestTone.Danger -> Icons.Rounded.Cancel
        PullRequestTone.Warning -> Icons.Rounded.Schedule
        else -> Icons.Rounded.RemoveCircleOutline
    }

/** Checks, reviewers and branch state in one card, each line expanding in place. */
@Composable
private fun PullRequestStatusCard(detail: PullRequestDetailDto, activity: PullRequestActivityDto?, onOpenUrl: (String) -> Unit) {
    var checksExpanded by rememberSaveable { mutableStateOf(false) }
    var reviewersExpanded by rememberSaveable { mutableStateOf(false) }
    val checks = remember(detail.checks) { pullRequestChecksSummary(detail.checks) }
    val review = remember(detail.reviewers, activity) { pullRequestReviewSummary(detail.reviewers, activity) }
    val providerLabel = openOnProviderLabel(detail.provider).removePrefix("Open on ")
    S5Card(tone = club.touchtech.s5code.kotlin.design.component.S5CardTone.Receded) {
        OverviewRow(
            icon = toneIcon(checks.tone),
            title = checks.title,
            value = checks.count,
            tint = checks.tone.color(),
            expanded = checksExpanded,
            onClick = { checksExpanded = !checksExpanded },
        ) {
            Column(Modifier.padding(horizontal = S5Theme.spacing.large), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                if (detail.checks.isEmpty()) {
                    Text("No checks reported.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                sortedPullRequestChecks(detail.checks).forEach { check ->
                    val status = checkStatus(check)
                    Row(
                        Modifier.fillMaxWidth()
                            .then(check.url?.let { url -> Modifier.clickable { onOpenUrl(url) } } ?: Modifier),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        Icon(toneIcon(status.tone), contentDescription = status.label, tint = status.tone.color(), modifier = Modifier.size(16.dp))
                        Column(Modifier.weight(1f)) {
                            Text(check.name, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            check.description?.takeIf { it.isNotBlank() }?.let {
                                Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                        Text(
                            status.label + if (check.required == true) " · Required" else "",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (detail.url.isNotEmpty()) {
                    S5Button(
                        text = "Open checks on $providerLabel",
                        onClick = { onOpenUrl(pullRequestChecksUrl(detail.provider, detail.url)) },
                        style = S5ButtonStyle.Text,
                        icon = Icons.AutoMirrored.Rounded.OpenInNew,
                    )
                }
            }
        }
        OverviewRow(
            icon = if (review.tone == PullRequestTone.Muted) Icons.Rounded.Group else toneIcon(review.tone),
            title = review.title,
            value = review.count,
            tint = review.tone.color(),
            expanded = reviewersExpanded,
            onClick = { reviewersExpanded = !reviewersExpanded },
        ) {
            Column(Modifier.padding(horizontal = S5Theme.spacing.large), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                val reviewed = review.reviews.mapNotNull { it.author?.login }.toSet()
                review.reviews.forEach { comment ->
                    ReviewerLine(
                        comment.author,
                        when (comment.reviewState) {
                            "APPROVED" -> "Approved"
                            "CHANGES_REQUESTED" -> "Requested changes"
                            else -> "Commented"
                        },
                    )
                }
                detail.reviewers.filter { it.login !in reviewed }.forEach { ReviewerLine(it, "Requested") }
                if (review.reviews.isEmpty() && detail.reviewers.isEmpty()) {
                    Text("No reviewers yet.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (detail.url.isNotEmpty()) {
                    S5Button(
                        text = "View reviewers on $providerLabel",
                        onClick = { onOpenUrl(detail.url) },
                        style = S5ButtonStyle.Text,
                        icon = Icons.AutoMirrored.Rounded.OpenInNew,
                    )
                }
            }
        }
        OverviewRow(
            icon = if (detail.mergeability == "conflicting") Icons.Rounded.Error else Icons.AutoMirrored.Rounded.CallSplit,
            title = "Branch",
            value = pullRequestBranchSummary(detail),
            tint = if (detail.mergeability == "conflicting") S5Theme.status.failed else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ReviewerLine(actor: PullRequestActorDto?, verdict: String) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        PullRequestAvatar(actor, size = 20.dp)
        Text(actor?.login ?: "Unknown reviewer", style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f))
        Text(verdict, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PullRequestLabels(detail: PullRequestDetailDto) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        detail.labels.forEach { label ->
            Row(
                Modifier.clip(MaterialTheme.shapes.small)
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                    .padding(horizontal = S5Theme.spacing.small, vertical = S5Theme.spacing.tiny),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                Box(
                    Modifier.size(8.dp)
                        .clip(CircleShape)
                        .background(pullRequestLabelColor(label.color)?.let { Color(it) } ?: MaterialTheme.colorScheme.outline)
                )
                Text(label.name, style = MaterialTheme.typography.labelMedium)
            }
        }
    }
}

/**
 * Markdown that clips at [collapsedHeight] behind "Show more" when it runs
 * longer. The body measures unbounded inside the clip, so the toggle appears
 * only when there is something to reveal.
 */
@Composable
fun CollapsibleMarkdown(source: String, collapsedHeight: Dp, modifier: Modifier = Modifier) {
    var expanded by rememberSaveable(source) { mutableStateOf(false) }
    var fullHeight by remember(source) { mutableIntStateOf(0) }
    val limit = with(LocalDensity.current) { collapsedHeight.roundToPx() }
    val canCollapse = fullHeight > limit
    Column(modifier) {
        Box(
            Modifier.fillMaxWidth()
                .then(if (canCollapse && !expanded) Modifier.heightIn(max = collapsedHeight) else Modifier)
                .clipToBounds()
        ) {
            S5Markdown(
                source = source,
                modifier = Modifier.wrapContentHeight(align = Alignment.Top, unbounded = true).onSizeChanged { fullHeight = it.height },
            )
        }
        if (canCollapse) {
            S5Button(
                text = if (expanded) "Show less" else "Show more",
                onClick = { expanded = !expanded },
                style = S5ButtonStyle.Text,
                trailingIcon = if (expanded) Icons.Rounded.ExpandLess else Icons.Rounded.ExpandMore,
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
        }
    }
}

@Composable
private fun PullRequestDescription(body: String) {
    val attachments = remember(body) { pullRequestAttachments(body) }
    var preview by remember { mutableStateOf<String?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        Text("Description", style = MaterialTheme.typography.titleSmallEmphasized)
        if (body.isBlank()) {
            Text("No description provided.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            CollapsibleMarkdown(body, DESCRIPTION_COLLAPSED_HEIGHT.dp)
        }
        if (attachments.isNotEmpty()) {
            Text("Attachments", style = MaterialTheme.typography.titleSmallEmphasized, modifier = Modifier.padding(top = S5Theme.spacing.small))
            LazyRow(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
                items(attachments, key = { it.uri }) { attachment ->
                    Column(Modifier.size(width = 160.dp, height = 132.dp)) {
                        AsyncImage(
                            model = attachment.uri,
                            contentDescription = attachment.caption,
                            contentScale = ContentScale.Crop,
                            modifier =
                                Modifier.size(width = 160.dp, height = 108.dp)
                                    .clip(MaterialTheme.shapes.medium)
                                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                                    .clickable { preview = attachment.uri },
                        )
                        Text(
                            attachment.caption,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
    }
    preview?.let { S5ImageLightbox(model = it, contentDescription = null, onDismiss = { preview = null }) }
}
