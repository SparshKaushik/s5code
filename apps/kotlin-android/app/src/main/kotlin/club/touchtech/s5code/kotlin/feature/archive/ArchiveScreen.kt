package club.touchtech.s5code.kotlin.feature.archive

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Archive
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Unarchive
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5SearchField
import club.touchtech.s5code.kotlin.design.component.S5TopBarProminence
import club.touchtech.s5code.kotlin.design.component.S5WaitState
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.connections.waitNotice
import club.touchtech.s5code.kotlin.feature.home.ThreadRow
import kotlinx.coroutines.launch

/** Archived threads with search, restore, and delete. */
@Composable
fun ArchiveScreen(
    store: AppStore,
    onBack: () -> Unit,
    onOpenThread: (String, String) -> Unit,
    confirmController: S5ConfirmDialogController,
) {
    val archived by store.workspace.archived.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    var query by rememberSaveable { mutableStateOf("") }
    var menuFor by remember { mutableStateOf<String?>(null) }

    val filtered =
        remember(archived, query) {
            val needle = query.trim().lowercase()
            if (needle.isEmpty()) archived
            else
                archived.filter { thread ->
                    thread.title.lowercase().contains(needle) ||
                        thread.branch.orEmpty().lowercase().contains(needle)
                }
        }

    val wait =
        remember(environments, archived.isEmpty()) {
            waitNotice(
                states = environments.map { it.state },
                environmentLabel = environments.singleOrNull()?.label,
                resourceName = "archive",
                hasContent = archived.isNotEmpty(),
            )
        }

    S5Screen(
        title = "Archived",
        subtitle = "${archived.size} threads",
        prominence = S5TopBarProminence.Section,
        onBack = onBack,
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            S5SearchField(
                value = query,
                onValueChange = { query = it },
                placeholder = "Search archived",
                modifier =
                    Modifier.padding(
                        horizontal = S5Theme.spacing.gutter,
                        vertical = S5Theme.spacing.small,
                    ),
            )
            if (filtered.isEmpty() && wait != null && query.isBlank()) {
                // "Nothing archived" on an environment that has not answered yet is a
                // claim about data nobody has read.
                S5WaitState(
                    title = wait.title,
                    detail = wait.detail,
                    icon = Icons.Rounded.Archive,
                    spinning = wait.spinning,
                )
            } else if (filtered.isEmpty()) {
                S5EmptyState(
                    icon = Icons.Rounded.Archive,
                    title = if (archived.isEmpty()) "Nothing archived" else "No matches",
                    detail =
                        if (archived.isEmpty()) {
                            "Archived threads stay on the machine and can be restored any time."
                        } else {
                            "Nothing matches \"$query\"."
                        },
                    actionLabel = if (query.isNotBlank()) "Clear search" else null,
                    onAction = if (query.isNotBlank()) ({ query = "" }) else null,
                )
            } else {
                LazyColumn(
                    Modifier.fillMaxSize(),
                    contentPadding =
                        PaddingValues(
                            start = S5Theme.spacing.gutter,
                            end = S5Theme.spacing.gutter,
                            bottom = 32.dp,
                        ),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    items(filtered, key = { it.id.value }) { thread ->
                        Column(Modifier.animateItem()) {
                        ThreadRow(
                            thread = thread,
                            project =
                                projects.firstOrNull {
                                    it.id == thread.projectId &&
                                        it.environmentId == thread.environmentId
                                },
                            environmentLabel = null,
                            resolveProjectIconUrl = store.workspace::projectIconUrl,
                            onClick = { onOpenThread(thread.environmentId.value, thread.id.value) },
                            trailing = {
                                S5OverflowMenu(
                                    icon = Icons.Rounded.Unarchive,
                                    label = "Archive actions",
                                    expanded = menuFor == thread.id.value,
                                    onExpandedChange = { open ->
                                        menuFor = if (open) thread.id.value else null
                                    },
                                    options =
                                        listOf(
                                            S5MenuOption("restore", "Restore", Icons.Rounded.Unarchive),
                                            S5MenuOption(
                                                "delete",
                                                "Delete",
                                                Icons.Rounded.Delete,
                                                destructive = true,
                                            ),
                                        ),
                                    onSelect = { action ->
                                        when (action) {
                                            "restore" ->
                                                scope.launch {
                                                    try {
                                                        store.workspace.setArchived(
                                                            thread.environmentId,
                                                            thread.id,
                                                            false,
                                                        )
                                                    } catch (error: Exception) {
                                                        store.showError(
                                                            error.message ?: "The thread could not be restored."
                                                        )
                                                    }
                                                }
                                            "delete" ->
                                                confirmController.show(
                                                    S5ConfirmDialogRequest(
                                                        title = "Permanently delete thread?",
                                                        message =
                                                            "\"${thread.title}\" cannot be restored after this.",
                                                        confirmText = "Delete",
                                                        destructive = true,
                                                        onConfirm = {
                                                            scope.launch {
                                                                try {
                                                                    store.workspace.deleteThread(
                                                                        thread.environmentId,
                                                                        thread.id,
                                                                    )
                                                                } catch (error: Exception) {
                                                                    store.showError(
                                                                        error.message ?: "The thread could not be deleted."
                                                                    )
                                                                }
                                                            }
                                                        },
                                                    )
                                                )
                                        }
                                    },
                                )
                            },
                        )
                        }
                    }
                }
            }
        }
    }
}
