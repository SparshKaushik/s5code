package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Group
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.PullRequestQuery
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.PullRequestReviewerRequest
import club.touchtech.s5code.kotlin.data.filterReviewerCandidates
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5LoadingState
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewerCandidateDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestReviewerCandidateListDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/**
 * `PullRequestReviewersScreen`: request or remove reviews. Hosts that list
 * candidates get a searchable list; the rest take a typed reviewer ID.
 */
@Composable
fun PullRequestReviewersScreen(
    store: AppStore,
    environmentId: String,
    reference: PullRequestReference,
    onBack: () -> Unit,
) {
    val gateway = store.pullRequests
    val scope = rememberCoroutineScope()
    val detailQuery = rememberPullRequestDetail(store, environmentId, reference)
    val pr = detailQuery.value
    val allowed = rememberPullRequestPermissions(store, environmentId, pr).canRequestReviewers
    val listCandidates = pr?.capabilities?.reviewers?.listCandidates == true
    val result by remember(environmentId, reference, listCandidates) {
            if (listCandidates) gateway.reviewerCandidates(environmentId, reference) else flowOf(PullRequestQuery())
        }
        .collectAsStateWithLifecycle(PullRequestQuery<PullRequestReviewerCandidateListDto>())
    var query by rememberSaveable { mutableStateOf("") }
    var pending by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val candidates = remember(result.value, query) { filterReviewerCandidates(result.value?.candidates.orEmpty(), query) }

    fun update(id: String, kind: String, isRequested: Boolean) {
        if (!allowed || pending != null) return
        pending = "$kind:$id"
        error = null
        scope.launch {
            try {
                gateway.requestReviewers(environmentId, reference, listOf(PullRequestReviewerRequest(id, kind)), !isRequested)
                if (!listCandidates) query = ""
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                error = failure.message ?: "Couldn't update this review request. Try again."
            } finally {
                pending = null
            }
        }
    }

    S5Screen(
        title = "Request review",
        subtitle = "#${reference.number} · ${reference.repository}",
        onBack = onBack,
        loading = result.loading && result.hasValue,
    ) { padding ->
        when {
            pr == null && detailQuery.error != null ->
                S5EmptyState(
                    icon = Icons.Rounded.Group,
                    title = "Couldn't load this pull request",
                    detail = detailQuery.error.orEmpty(),
                    modifier = Modifier.padding(padding),
                )
            pr == null -> S5LoadingState("Loading pull request", Modifier.padding(padding))
            listCandidates ->
                Column(Modifier.fillMaxSize().padding(padding).imePadding()) {
                    Column(
                        Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.medium),
                        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    ) {
                        MutedText("Tap someone to request a review. Tap a requested reviewer to remove the request.")
                        S5SearchField(value = query, onValueChange = { query = it }, placeholder = "Search reviewers")
                        if (!allowed) MutedText("Requesting reviews requires write access and an open pull request.")
                        error?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error) }
                    }
                    Box(Modifier.weight(1f)) {
                        when {
                            candidates.isNotEmpty() ->
                                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = S5Theme.spacing.xxLarge)) {
                                    items(candidates, key = { "${it.kind}:${it.id}" }) { candidate ->
                                        ReviewerCandidateRow(
                                            candidate = candidate,
                                            pending = pending == "${candidate.kind}:${candidate.id}",
                                            enabled = allowed && pending == null,
                                            onClick = { update(candidate.id, candidate.kind, candidate.isRequested) },
                                        )
                                    }
                                    if (result.value?.truncated == true) {
                                        item(key = "truncated") {
                                            MutedText(
                                                "The provider has more reviewers than it returned. Find additional reviewers on the provider.",
                                                Modifier.padding(S5Theme.spacing.gutter),
                                            )
                                        }
                                    }
                                }
                            !result.hasValue && result.error == null -> S5LoadingState("Loading reviewers")
                            else ->
                                S5EmptyState(
                                    icon = Icons.Rounded.Group,
                                    title =
                                        when {
                                            result.error != null -> "Couldn't load reviewers"
                                            query.isNotBlank() -> "No matching reviewers"
                                            else -> "No reviewers available"
                                        },
                                    detail = result.error ?: "Search people and teams with access to this repository.",
                                    actionLabel = if (result.error != null) "Retry" else null,
                                    onAction = { scope.launch { gateway.refreshReviewerCandidates(environmentId, reference) } },
                                )
                        }
                    }
                }
            else ->
                Column(
                    Modifier.fillMaxSize()
                        .padding(padding)
                        .imePadding()
                        .verticalScroll(rememberScrollState())
                        .padding(S5Theme.spacing.gutter),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                ) {
                    MutedText("This provider accepts a reviewer ID but cannot list candidates.")
                    S5TextField(
                        value = query,
                        onValueChange = { query = it },
                        label = "Reviewer ID",
                        placeholder = "Reviewer login or ID",
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    S5Button(
                        text = if (pending != null) "Updating…" else "Request review",
                        enabled = allowed && query.isNotBlank() && pending == null,
                        onClick = { update(query.trim(), "user", isRequested = false) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    S5Button(
                        text = "Remove review request",
                        style = S5ButtonStyle.Text,
                        enabled = allowed && query.isNotBlank() && pending == null,
                        onClick = { update(query.trim(), "user", isRequested = true) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    if (!allowed) MutedText("Requesting reviews requires write access and an open pull request.")
                    error?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error) }
                }
        }
    }
}

@Composable
private fun MutedText(message: String, modifier: Modifier = Modifier) {
    Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier)
}

@Composable
private fun ReviewerCandidateRow(
    candidate: PullRequestReviewerCandidateDto,
    pending: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Column {
        Row(
            Modifier.fillMaxWidth()
                .heightIn(min = 64.dp)
                .clickable(
                    enabled = enabled,
                    onClickLabel = if (candidate.isRequested) "Remove review request" else "Request review",
                    onClick = onClick,
                )
                .padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            PullRequestAvatar(candidate.actor, size = 32.dp)
            Column(Modifier.weight(1f)) {
                Text(
                    candidate.name?.takeIf(String::isNotBlank) ?: candidate.login,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    candidate.login + if (candidate.kind == "team") " · Team" else "",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            when {
                pending -> S5InlineLoading()
                candidate.isRequested ->
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                        Text("Requested", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        Icon(Icons.Rounded.Check, contentDescription = null, modifier = Modifier.size(16.dp))
                    }
                else ->
                    Icon(
                        Icons.Rounded.Add,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(18.dp),
                    )
            }
        }
        HorizontalDivider(Modifier.padding(start = S5Theme.spacing.gutter), color = MaterialTheme.colorScheme.outlineVariant)
    }
}
