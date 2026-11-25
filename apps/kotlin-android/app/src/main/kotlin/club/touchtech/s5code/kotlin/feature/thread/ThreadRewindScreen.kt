package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Restore
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.design.component.S5Notice
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5StatusPill
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ThreadId
import kotlinx.coroutines.launch

/** Checkpoint list with preview and rewind. */
@Composable
fun ThreadRewindScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    onBack: () -> Unit,
    confirmController: S5ConfirmDialogController,
) {
    val id = remember(threadId) { ThreadId(threadId) }
    val env = remember(environmentId) { EnvironmentId(environmentId) }
    val detail by
        remember(environmentId, threadId) { store.workspace.thread(env, id) }
            .collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    var selected by remember(threadId) { mutableStateOf<String?>(null) }
    var rewinding by remember(threadId) { mutableStateOf(false) }
    val checkpoints = detail?.checkpoints.orEmpty()

    S5Screen(
        title = "Rewind",
        subtitle = "${checkpoints.size} checkpoints",
        onBack = onBack,
        loading = rewinding,
    ) { padding ->
        if (checkpoints.isEmpty()) {
            S5EmptyState(
                icon = Icons.Rounded.History,
                title = "No checkpoints yet",
                detail = "Each completed turn creates a checkpoint you can return to.",
                modifier = Modifier.padding(padding),
            )
        } else {
            Column(Modifier.fillMaxSize().padding(padding)) {
                Box(Modifier.padding(horizontal = S5Theme.spacing.gutter)) {
                    S5Notice(
                        icon = Icons.Rounded.Restore,
                        text = "Rewinding restores files and drops transcript entries after that point.",
                    )
                }
                LazyColumn(
                    Modifier.weight(1f),
                    contentPadding =
                        PaddingValues(horizontal = S5Theme.spacing.gutter, vertical = S5Theme.spacing.small),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                ) {
                    items(checkpoints, key = { it.id }) { checkpoint ->
                        Box(Modifier.animateItem()) {
                        S5Card(
                            tone = if (checkpoint.current) S5CardTone.Hero else S5CardTone.Standard,
                            onClick = { selected = checkpoint.id },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Row(
                                Modifier.padding(S5Theme.spacing.large),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
                            ) {
                                Column(Modifier.weight(1f)) {
                                    Text(
                                        checkpoint.label,
                                        style = MaterialTheme.typography.titleSmallEmphasized,
                                    )
                                    Text(
                                        "${checkpoint.timeLabel} · ${checkpoint.filesChanged} files",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                if (checkpoint.current) {
                                    S5StatusPill(
                                        label = "Current",
                                        containerColor = S5Theme.status.settledContainer,
                                        contentColor = S5Theme.status.onSettledContainer,
                                    )
                                } else if (selected == checkpoint.id) {
                                    // Tapping a card selects it and this button is the
                                    // confirm: reverting drops newer turns, so it takes
                                    // two deliberate taps rather than one.
                                    S5Button(
                                        text = "Rewind here",
                                        onClick = {
                                            val turnCount = checkpoint.id.toIntOrNull()
                                            if (turnCount != null) {
                                                confirmController.show(
                                                    S5ConfirmDialogRequest(
                                                        title = "Rewind to this checkpoint?",
                                                        message =
                                                            "Files will be restored and newer transcript entries will be dropped.",
                                                        confirmText = "Rewind",
                                                        destructive = true,
                                                        onConfirm = {
                                                            val actionId =
                                                                store.beginAction(
                                                                    "Rewinding thread",
                                                                    "Restoring ${checkpoint.label}",
                                                                )
                                                            rewinding = true
                                                            scope.launch {
                                                                runCatching {
                                                                    store.workspace.revertToCheckpoint(
                                                                        env,
                                                                        id,
                                                                        turnCount,
                                                                    )
                                                                }.fold(
                                                                    onSuccess = {
                                                                        store.finishAction(
                                                                            actionId,
                                                                            "Thread rewound",
                                                                            checkpoint.label,
                                                                        )
                                                                        onBack()
                                                                    },
                                                                    onFailure = { cause ->
                                                                        store.failAction(
                                                                            actionId,
                                                                            "Rewind failed",
                                                                            cause.message
                                                                                ?: "The checkpoint could not be restored.",
                                                                        )
                                                                    },
                                                                )
                                                                rewinding = false
                                                            }
                                                        },
                                                    )
                                                )
                                            }
                                        },
                                        enabled = !rewinding,
                                        emphasis = S5ActionEmphasis.Prominent,
                                        icon = Icons.Rounded.Restore,
                                    )
                                }
                            }
                        }
                        }
                    }
                }
            }
        }
    }
}
