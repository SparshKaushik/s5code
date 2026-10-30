package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import club.touchtech.s5code.kotlin.data.proposedPlanFileContents
import club.touchtech.s5code.kotlin.data.proposedPlanTitle
import club.touchtech.s5code.kotlin.data.stripDisplayedPlanMarkdown
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5Markdown
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.FeedEntry
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/**
 * The plan an agent proposed, ported from RN's `ProposedPlanCard`: its heading as
 * the title, the rest as Markdown, and a way to keep it as a workspace file.
 *
 * [onSavePlan] writes `projects.writeFile` and returns the saved relative path;
 * null when the thread has no workspace or the connection cannot write files.
 */
@Composable
internal fun ProposedPlanCard(
    entry: FeedEntry.ProposedPlan,
    onCopy: (String) -> Unit,
    modifier: Modifier,
    workspaceRoot: String?,
    onOpenFile: (String) -> Unit,
    onOpenThreadLink: ((String) -> Unit)?,
    onSavePlan: (suspend (relativePath: String, contents: String) -> String)?,
) {
    val contents = remember(entry.markdown) { proposedPlanFileContents(entry.markdown) }
    val title = remember(entry.markdown) { proposedPlanTitle(entry.markdown) ?: "Proposed plan" }
    val body = remember(entry.markdown) { stripDisplayedPlanMarkdown(entry.markdown) }
    val scope = rememberCoroutineScope()
    var saving by remember(entry.id) { mutableStateOf(false) }
    var prompting by remember(entry.id) { mutableStateOf(false) }
    var lastPath by remember(entry.id) { mutableStateOf("plan.md") }
    var outcome by remember(entry.id) { mutableStateOf<String?>(null) }

    S5Card(tone = S5CardTone.Standard, modifier = modifier.fillMaxWidth()) {
        Column(
            Modifier.padding(S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f).semantics { heading() },
                )
                S5IconButton(
                    icon = Icons.Rounded.ContentCopy,
                    label = "Copy plan",
                    onClick = { onCopy(contents) },
                )
            }
            S5Markdown(
                source = body,
                onCopyCode = onCopy,
                workspaceRoot = workspaceRoot,
                onOpenFile = onOpenFile,
                onOpenThreadLink = onOpenThreadLink,
            )
            S5Button(
                text = if (saving) "Saving..." else "Save to workspace",
                onClick = { prompting = true },
                emphasis = S5ActionEmphasis.Secondary,
                style = S5ButtonStyle.Tonal,
                enabled = onSavePlan != null && !saving && !entry.streaming,
            )
            outcome?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    if (prompting && onSavePlan != null) {
        var path by remember { mutableStateOf(lastPath) }
        AlertDialog(
            onDismissRequest = { prompting = false },
            title = { Text("Save plan to workspace") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                    // The write replaces an existing file; the RPC has no create-only mode.
                    Text(
                        "Enter a path relative to the thread's workspace. " +
                            "A file already at that path is replaced.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    S5TextField(value = path, onValueChange = { path = it }, singleLine = true)
                }
            },
            dismissButton = { TextButton(onClick = { prompting = false }) { Text("Cancel") } },
            confirmButton = {
                TextButton(
                    enabled = path.isNotBlank(),
                    onClick = {
                        val relativePath = path.trim()
                        prompting = false
                        saving = true
                        lastPath = relativePath
                        outcome = null
                        scope.launch {
                            outcome =
                                try {
                                    "Plan saved to ${onSavePlan(relativePath, contents)}"
                                } catch (cancellation: CancellationException) {
                                    throw cancellation
                                } catch (error: Exception) {
                                    "Could not save plan: ${error.message ?: "the write failed."}"
                                } finally {
                                    saving = false
                                }
                        }
                    },
                ) { Text("Save") }
            },
        )
    }
}
