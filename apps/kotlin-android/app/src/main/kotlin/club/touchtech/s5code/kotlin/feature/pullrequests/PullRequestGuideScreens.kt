package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.FormatListNumbered
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Description
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.Visibility
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.data.GuideDiffFocus
import club.touchtech.s5code.kotlin.data.GuideNoticeKind
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.combinePullRequestDiffPages
import club.touchtech.s5code.kotlin.data.excerptFile
import club.touchtech.s5code.kotlin.data.guideNoticeState
import club.touchtech.s5code.kotlin.data.letterStopNotes
import club.touchtech.s5code.kotlin.data.reviewGuideState
import club.touchtech.s5code.kotlin.data.stepTourPosition
import club.touchtech.s5code.kotlin.data.stopFilePaths
import club.touchtech.s5code.kotlin.data.timeLabel
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.review.ReviewFileCard
import club.touchtech.s5code.kotlin.model.ReviewFile
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideExcerptDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewGuideResultDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/* Guided review, ported from RN's `guide` folder: the PR page's Guide segment
 * (notice and stops outline) and one screen per stop. */

private const val GUIDE_POLL_MS = 5_000L

private val ATTENTION_LABELS =
    mapOf(
        "tradeoff" to "A tradeoff to weigh",
        "contract" to "A contract to check",
        "product" to "A product call",
        "risk" to "A risk to judge",
        "accessibility" to "An accessibility call",
    )

/** `useReviewGuide`: the saved guide, and writing a new one. */
@Stable
class ReviewGuideController internal constructor(
    /** The server's `reviewGuidesEnabled` setting; nothing is fetched while it is off. */
    val enabled: Boolean,
    val query: PullRequestQuery<PullRequestReviewGuideResultDto>,
    /** Connected, with the `orchestration:operate` grant writing needs. */
    val canWrite: Boolean,
    val running: Boolean,
    val startError: String?,
    private val onWrite: () -> Unit,
    private val onRefresh: () -> Unit,
) {
    val data: PullRequestReviewGuideResultDto? get() = query.value.takeIf { query.hasValue }
    val notice get() = guideNoticeState(data, running, startError, query.error)

    fun write() {
        if (enabled && canWrite && !running) onWrite()
    }

    fun refresh() = onRefresh()
}

@Composable
fun rememberReviewGuide(store: AppStore, environmentId: String, reference: PullRequestReference): ReviewGuideController {
    val gateway = store.pullRequests
    val scope = rememberCoroutineScope()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val environment = environments.firstOrNull { it.id.value == environmentId }
    val enabled = environment?.serverSettings?.reviewGuidesEnabled == true
    val query by remember(environmentId, reference, enabled) {
            if (enabled) gateway.reviewGuide(environmentId, reference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestReviewGuideResultDto>())
    val connected = rememberPullRequestConnected(store, environmentId)
    val canWrite = connected && environment?.capabilities?.sessionAllowsOperate == true
    var starting by remember(environmentId, reference) { mutableStateOf(false) }
    var startError by remember(environmentId, reference) { mutableStateOf<String?>(null) }
    val running = starting || query.value?.run?.tag == "Running"
    // Poll while the agent writes, and only while this screen is showing.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(environmentId, reference, enabled, running) {
        if (!enabled || !running) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                delay(GUIDE_POLL_MS)
                gateway.refreshReviewGuide(environmentId, reference)
            }
        }
    }
    return ReviewGuideController(
        enabled = enabled,
        query = query,
        canWrite = canWrite,
        running = running,
        startError = startError,
        onWrite = {
            if (!starting) {
                starting = true
                startError = null
                scope.launch {
                    try {
                        gateway.generateReviewGuide(environmentId, reference)
                    } catch (cancelled: CancellationException) {
                        throw cancelled
                    } catch (failure: Exception) {
                        startError = failure.message ?: "Could not start the review guide."
                    } finally {
                        starting = false
                    }
                }
            }
        },
        onRefresh = { scope.launch { gateway.refreshReviewGuide(environmentId, reference) } },
    )
}

/** The Guide segment of a PR page: the notice, then the stops outline once a guide exists. */
fun LazyListScope.pullRequestGuideItems(guide: ReviewGuideController, onOpenStop: (position: Int, guide: PullRequestReviewGuideDto) -> Unit) {
    val state = reviewGuideState(guide.data)
    val notice = guide.notice
    if (notice.kind != GuideNoticeKind.Ready) item(key = "guide-notice") { GuideNoticeCard(guide) }
    val saved = state.guide ?: return
    val canRewrite = guide.canWrite && !guide.running && !state.stale && guide.startError == null && guide.query.error == null
    item(key = "guide-outline-header") {
        val files = remember(saved) { saved.stops.flatMap(::stopFilePaths).toSet().size }
        var menuOpen by remember { mutableStateOf(false) }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
            Text("${saved.stops.size} stops", style = MaterialTheme.typography.titleSmallEmphasized)
            Text(
                "through $files ${if (files == 1) "file" else "files"}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            if (canRewrite) {
                S5OverflowMenu(
                    icon = Icons.Rounded.MoreVert,
                    label = "Guide options",
                    options = listOf(S5MenuOption("rewrite", "Rewrite guide", Icons.Rounded.Refresh)),
                    onSelect = { guide.write() },
                    expanded = menuOpen,
                    onExpandedChange = { menuOpen = it },
                )
            }
        }
    }
    saved.stops.firstOrNull()?.summary?.takeIf(String::isNotBlank)?.let { summary ->
        item(key = "guide-summary") { S5Markdown(summary) }
    }
    itemsIndexed(saved.stops, key = { position, _ -> "guide-stop:${saved.headSha}:${saved.generatedAt}:$position" }) { position, stop ->
        val paths = stopFilePaths(stop)
        Row(
            Modifier.fillMaxWidth().height(IntrinsicSize.Min).clickable(onClickLabel = "Open stop") { onOpenStop(position, saved) },
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Column(Modifier.width(32.dp).fillMaxHeight(), horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    Modifier.size(32.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                        .border(1.dp, MaterialTheme.colorScheme.outlineVariant, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("${position + 1}", style = MaterialTheme.typography.labelLarge)
                }
                if (position < saved.stops.size - 1) {
                    Box(Modifier.width(1.dp).weight(1f).background(MaterialTheme.colorScheme.outlineVariant))
                }
            }
            Column(
                Modifier.weight(1f).padding(top = 4.dp, bottom = S5Theme.spacing.xLarge),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                Text(stop.title, style = MaterialTheme.typography.bodyLarge)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                    if (stop.attention != null) {
                        Icon(
                            Icons.Rounded.Visibility,
                            contentDescription = "Needs attention",
                            tint = S5Theme.status.approval,
                            modifier = Modifier.size(14.dp),
                        )
                    }
                    Text(
                        (paths.firstOrNull()?.substringAfterLast('/') ?: "Context") +
                            if (paths.size > 1) " +${paths.size - 1}" else "",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Icon(
                Icons.AutoMirrored.Rounded.KeyboardArrowRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 6.dp).size(18.dp),
            )
        }
    }
    if (saved.stops.isNotEmpty()) {
        item(key = "guide-start") {
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                S5Button(
                    text = "Start tour",
                    onClick = { onOpenStop(0, saved) },
                    style = S5ButtonStyle.Text,
                    trailingIcon = Icons.AutoMirrored.Rounded.ArrowForward,
                )
            }
        }
    }
}

/** `GuideNotice`: full-size before a guide exists, compact above one. */
@Composable
private fun GuideNoticeCard(guide: ReviewGuideController) {
    val notice = guide.notice
    val compact = guide.data?.guide != null
    val failed = notice.kind == GuideNoticeKind.Failed || notice.kind == GuideNoticeKind.Unavailable
    val busy = notice.kind == GuideNoticeKind.Loading || notice.kind == GuideNoticeKind.Writing
    S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(S5Theme.spacing.large), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                Icon(
                    when {
                        failed -> Icons.Rounded.ErrorOutline
                        busy -> Icons.Rounded.Schedule
                        else -> Icons.Rounded.FormatListNumbered
                    },
                    contentDescription = null,
                    tint = if (failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(20.dp),
                )
                Text(if (compact) notice.title else "Review guide", style = MaterialTheme.typography.titleSmallEmphasized)
            }
            Text(
                notice.description,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = if (compact) Modifier else Modifier.heightIn(min = 72.dp),
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
                if (guide.canWrite || busy || notice.kind == GuideNoticeKind.Unavailable) {
                    S5Button(
                        text = notice.action,
                        onClick = { if (notice.kind == GuideNoticeKind.Unavailable) guide.refresh() else guide.write() },
                        enabled = !busy,
                        style = if (compact) S5ButtonStyle.Tonal else S5ButtonStyle.Filled,
                        emphasis = S5ActionEmphasis.Secondary,
                    )
                } else {
                    Text(
                        if (notice.kind == GuideNoticeKind.Failed) {
                            "Reconnect or ask someone with write access to retry."
                        } else {
                            "Writing a guide requires a connection and write access."
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                val run = guide.data?.run
                if (notice.kind == GuideNoticeKind.Writing && run?.tag == "Running") {
                    timeLabel(run.startedAt).takeIf(String::isNotEmpty)?.let {
                        Text("Started $it", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }
    }
}

/**
 * `PullRequestGuideStopScreen`: one stop of the tour with its excerpts cut
 * from the PR's diff, lettered notes, files and questions, then the next stop.
 */
@Composable
fun PullRequestGuideStopScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    position: Int,
    guideHeadSha: String,
    guideGeneratedAt: String,
    onBack: () -> Unit,
    onOpenRoute: (String) -> Unit,
    /** Back to the PR page the tour started from. */
    onFinishTour: () -> Unit,
) {
    val guideState = rememberReviewGuide(store, environmentId, reference)
    val detail = rememberPullRequestDetail(store, environmentId, reference).value
    val permissions = rememberPullRequestPermissions(store, environmentId, detail)
    val canReview = permissions.verdicts.isNotEmpty() && detail?.headSha == guideHeadSha
    val state = reviewGuideState(guideState.data)
    val guide = state.guide
    val index = position.coerceIn(0, ((guide?.stops?.size ?: 1) - 1).coerceAtLeast(0))
    val stop = guide?.stops?.getOrNull(index)
    val changedGuide = guide != null && (guide.headSha != guideHeadSha || guide.generatedAt != guideGeneratedAt)

    val (diff, load) = rememberPullRequestDiff(store, environmentId, reference, null)
    val files = remember(diff.pages) { combinePullRequestDiffPages(diff.pages).files }
    val excerptFiles =
        remember(files) {
            buildMap<String, ReviewFile> {
                files.forEach { file ->
                    put(file.path, file)
                    file.oldPath?.let { put(it, file) }
                }
            }
        }
    val missing = stop != null && stopFilePaths(stop).any { path -> files.none { it.path == path } }
    LaunchedEffect(guideState.enabled, missing, diff.nextCursor, diff.loading, diff.error) {
        if (guideState.enabled && missing && diff.nextCursor != null && !diff.loading && diff.error == null) load(diff.nextCursor)
    }

    S5Screen(
        title = "Stop ${position + 1} of ${guide?.stops?.size ?: "…"}",
        subtitle = "#${reference.number} · ${reference.repository}",
        onBack = onBack,
        loading = diff.loading && diff.loaded,
    ) { padding ->
        when {
            !guideState.enabled -> GuideMessage("Review guides are disabled for this server.", Modifier.padding(padding))
            changedGuide -> GuideMessage("This guide was rewritten. Go back to start the new tour.", Modifier.padding(padding))
            stop == null ->
                guideState.query.error?.let { GuideMessage(it, Modifier.padding(padding)) }
                    ?: S5LoadingState("Loading stop", Modifier.padding(padding))
            else ->
                Column(
                    Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(S5Theme.spacing.gutter),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.large),
                ) {
                    if (guideState.notice.kind != GuideNoticeKind.Ready) GuideNoticeCard(guideState)
                    Text(stop.title, style = MaterialTheme.typography.headlineSmallEmphasized)
                    S5Markdown(stop.summary)
                    stop.attention?.let { attention ->
                        S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(S5Theme.spacing.large), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                                    Icon(Icons.Rounded.Visibility, contentDescription = null, modifier = Modifier.size(18.dp))
                                    Text(
                                        ATTENTION_LABELS[attention.kind] ?: "Worth a closer look",
                                        style = MaterialTheme.typography.titleSmallEmphasized,
                                    )
                                }
                                S5Markdown(attention.note)
                            }
                        }
                    }
                    if (state.stale) {
                        GuideMuted("Excerpts are hidden until the guide is rewritten for the current head.")
                    } else {
                        stop.excerpts.forEach { excerpt ->
                            GuideExcerpt(
                                excerpt = excerpt,
                                file = excerptFiles[excerpt.path],
                                loading = !diff.loaded || (missing && diff.nextCursor != null),
                                onOpen = {
                                    onOpenRoute(
                                        Routes.pullRequestDiff(
                                            environmentId,
                                            reference,
                                            path = excerptFiles[excerpt.path]?.path ?: excerpt.path,
                                            focus = GuideDiffFocus(excerpt.side, excerpt.start, excerpt.end),
                                        )
                                    )
                                },
                            )
                        }
                    }
                    diff.error?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                    val notes = remember(stop) { letterStopNotes(stop) }
                    if (notes.isNotEmpty()) {
                        Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
                            Text("Notes on this stop", style = MaterialTheme.typography.titleSmallEmphasized)
                            notes.forEach { (marker, note) ->
                                Row(horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium)) {
                                    Box(
                                        Modifier.size(28.dp).clip(MaterialTheme.shapes.small).background(MaterialTheme.colorScheme.surfaceContainerHigh),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Text(marker, style = MaterialTheme.typography.labelMedium)
                                    }
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                                        Text(note.title, style = MaterialTheme.typography.titleSmall)
                                        Text(
                                            "${note.path.substringAfterLast('/')}:${note.line}",
                                            style = S5Theme.code.inlineTechnical,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                        S5Markdown(note.body)
                                    }
                                }
                            }
                        }
                    }
                    if (stop.files.isNotEmpty()) {
                        S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth()) {
                            stop.files.forEachIndexed { fileIndex, file ->
                                Row(
                                    Modifier.fillMaxWidth()
                                        .clickable { onOpenRoute(Routes.pullRequestDiff(environmentId, reference, path = file.path)) }
                                        .padding(horizontal = S5Theme.spacing.large, vertical = S5Theme.spacing.medium),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                                ) {
                                    Icon(
                                        Icons.Rounded.Description,
                                        contentDescription = null,
                                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.size(20.dp),
                                    )
                                    Column(Modifier.weight(1f)) {
                                        Text(file.path, style = S5Theme.code.inlineTechnical, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                        file.note?.takeIf(String::isNotBlank)?.let { GuideMuted(it) }
                                    }
                                }
                                if (fileIndex < stop.files.size - 1) HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                            }
                        }
                    }
                    if (stop.questions.isNotEmpty()) {
                        Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                            Text("Questions to consider", style = MaterialTheme.typography.titleSmallEmphasized)
                            stop.questions.forEach { S5Markdown(it) }
                        }
                    }
                    val stopCount = guide.stops.size
                    val hasNext = index < stopCount - 1
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable {
                                when {
                                    hasNext ->
                                        onOpenRoute(
                                            Routes.pullRequestGuideStop(
                                                environmentId,
                                                reference,
                                                stepTourPosition(index, 1, stopCount),
                                                guideHeadSha,
                                                guideGeneratedAt,
                                            )
                                        )
                                    canReview -> onOpenRoute(Routes.pullRequestReview(environmentId, reference, "review"))
                                    else -> onFinishTour()
                                }
                            }
                            .padding(vertical = S5Theme.spacing.medium),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.large),
                    ) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                            GuideMuted(if (hasNext) "Next · ${index + 2} of $stopCount" else "End of guide")
                            Text(
                                when {
                                    hasNext -> guide.stops[index + 1].title
                                    canReview -> "Finish review"
                                    else -> "Finish tour"
                                },
                                style = MaterialTheme.typography.titleMedium,
                            )
                        }
                        Box(
                            Modifier.size(44.dp).clip(CircleShape).background(MaterialTheme.colorScheme.onSurface),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                if (hasNext) Icons.AutoMirrored.Rounded.ArrowForward else Icons.Rounded.Check,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.surface,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                    }
                }
        }
    }
}

@Composable
private fun GuideMessage(message: String, modifier: Modifier = Modifier) {
    Text(message, style = MaterialTheme.typography.bodyMedium, modifier = modifier.padding(S5Theme.spacing.gutter))
}

@Composable
private fun GuideMuted(message: String) {
    Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

/** `ReviewGuideExcerpt`: the caption, then the excerpt's lines; tapping either opens the full diff there. */
@Composable
private fun GuideExcerpt(excerpt: PullRequestReviewGuideExcerptDto, file: ReviewFile?, loading: Boolean, onOpen: () -> Unit) {
    val slice = remember(file, excerpt) { file?.let { excerptFile(it, excerpt) } }
    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        if (excerpt.caption.isNotBlank()) S5Markdown(excerpt.caption)
        if (slice != null) {
            ReviewFileCard(
                file = slice,
                expanded = true,
                selectedRange = null,
                rangeAnchorIndex = null,
                onLineTap = { _, _ -> onOpen() },
                onLineLongPress = null,
                onToggle = onOpen,
                headerTrailing = {
                    Text(
                        "${excerpt.start}–${excerpt.end}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Icon(
                        Icons.AutoMirrored.Rounded.OpenInNew,
                        contentDescription = "Open in full diff",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp),
                    )
                },
            )
        } else {
            S5Card(tone = S5CardTone.Receded, modifier = Modifier.fillMaxWidth(), onClick = onOpen) {
                Text(
                    if (loading) "Loading ${excerpt.path}…" else "${excerpt.path}:${excerpt.start}–${excerpt.end} is not in the loaded diff.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(S5Theme.spacing.medium),
                )
            }
        }
    }
}
