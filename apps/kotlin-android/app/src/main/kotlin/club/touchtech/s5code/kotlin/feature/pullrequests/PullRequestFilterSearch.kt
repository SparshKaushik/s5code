package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.data.PullRequestGateway
import club.touchtech.s5code.kotlin.data.pullRequestFilterChoices
import club.touchtech.s5code.kotlin.data.pullRequestLabelColors
import club.touchtech.s5code.kotlin.design.component.S5BottomSheet
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.transport.wire.PullRequestFilterOptionsDto
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay

private const val FILTER_SEARCH_DEBOUNCE_MS = 250L
private const val FILTER_QUERY_MAX_CHARS = 100

/**
 * `PullRequestFilterSearchScreen`: a label or author picker that also asks the
 * host (`pullRequests.filterOptions`) for values no loaded row carries. A
 * server that predates the method answers with a failure, which leaves the
 * loaded values on their own.
 */
@Composable
fun PullRequestFilterSearchSheet(
    gateway: PullRequestGateway,
    /** labels | authors. */
    kind: String,
    environmentIds: List<String>,
    projectId: String?,
    /** What the loaded rows already carry, offered before the host answers. */
    suggestions: List<String>,
    selected: List<String>,
    labelColors: Map<String, Long>,
    onChoose: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var query by remember { mutableStateOf("") }
    val typed = query.trim().take(FILTER_QUERY_MAX_CHARS)
    var sent by remember { mutableStateOf<String?>(null) }
    var hosted by remember { mutableStateOf(emptyList<PullRequestFilterOptionsDto>()) }
    var failed by remember { mutableStateOf(false) }
    LaunchedEffect(typed, environmentIds, projectId, kind) {
        if (sent != null) delay(FILTER_SEARCH_DEBOUNCE_MS)
        val results =
            coroutineScope {
                environmentIds.map { environmentId ->
                    async {
                        try {
                            gateway.filterOptions(environmentId, kind, listOfNotNull(projectId), host = null, query = typed)
                        } catch (cancelled: CancellationException) {
                            throw cancelled
                        } catch (_: Exception) {
                            null
                        }
                    }
                }.awaitAll()
            }
        hosted = results.filterNotNull()
        failed = results.any { it == null }
        sent = typed
    }
    // Until the search catches up with the typing, the host's answers are for older text.
    val settled = sent == typed
    val rows =
        pullRequestFilterChoices(
            selected = selected,
            suggestions = suggestions,
            hosted =
                if (!settled) emptyList()
                else hosted.flatMap { options -> if (kind == "labels") options.labels.map { it.name } else options.authors.map { it.login } },
            query = query,
        )
    val colors = remember(hosted, labelColors) { pullRequestLabelColors(hosted.flatMap { it.labels }) + labelColors }
    fun isSelected(value: String) = selected.any { it.equals(value, ignoreCase = true) }

    S5BottomSheet(onDismiss = onDismiss, title = if (kind == "labels") "Filter by label" else "Filter by author") {
        S5SearchField(
            value = query,
            onValueChange = { query = it },
            placeholder = if (kind == "labels") "Search labels" else "Search authors",
            modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
        )
        if (failed && settled) {
            Text(
                "The host couldn't be searched. Showing values from loaded pull requests.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = S5Theme.spacing.gutter),
            )
        }
        when {
            rows.isEmpty() && !settled ->
                Box(Modifier.fillMaxWidth().padding(S5Theme.spacing.xxLarge), contentAlignment = Alignment.Center) { S5InlineLoading() }
            rows.isEmpty() ->
                Text(
                    if (kind == "labels") "No matching labels" else "No matching authors",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(S5Theme.spacing.gutter),
                )
            else ->
                LazyColumn(Modifier.fillMaxWidth().heightIn(max = 480.dp)) {
                    items(rows, key = { it.lowercase() }) { value ->
                        Row(
                            Modifier.fillMaxWidth()
                                .heightIn(min = 52.dp)
                                .clickable { onChoose(value) }
                                .padding(horizontal = S5Theme.spacing.gutter),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                        ) {
                            if (kind == "labels") {
                                Box(
                                    Modifier.size(10.dp)
                                        .clip(CircleShape)
                                        .background(colors[value.lowercase()]?.let { Color(it) } ?: MaterialTheme.colorScheme.outline)
                                )
                            }
                            Text(
                                value,
                                style = MaterialTheme.typography.bodyLarge,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            if (kind == "labels") {
                                Checkbox(checked = isSelected(value), onCheckedChange = null)
                            } else {
                                RadioButton(selected = isSelected(value), onClick = null)
                            }
                        }
                    }
                }
        }
    }
}
