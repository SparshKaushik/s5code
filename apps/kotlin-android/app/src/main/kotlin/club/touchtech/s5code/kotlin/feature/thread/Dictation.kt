package club.touchtech.s5code.kotlin.feature.thread

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.design.component.S5ComposerAction
import club.touchtech.s5code.kotlin.design.component.S5ComposerControl
import club.touchtech.s5code.kotlin.design.component.S5ComposerToolbarRow
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.platform.SpeechRecognizerVoiceEngine
import club.touchtech.s5code.kotlin.platform.VoiceComposerPresentation
import club.touchtech.s5code.kotlin.platform.VoiceDraftSnapshot
import club.touchtech.s5code.kotlin.platform.VoiceInputController
import club.touchtech.s5code.kotlin.platform.VoiceInputErrorAction
import club.touchtech.s5code.kotlin.platform.VoiceInputPhase
import club.touchtech.s5code.kotlin.platform.VoiceInputState
import club.touchtech.s5code.kotlin.platform.VoicePermissionResult
import club.touchtech.s5code.kotlin.platform.resolveVoiceComposerPresentation
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch

/**
 * A composer's dictation session: the controller plus its live state and the
 * toolbar presentation derived from it. Null when the device has no speech
 * recognizer — the mic is simply absent there.
 */
class Dictation internal constructor(
    val controller: VoiceInputController,
    val state: VoiceInputState,
    val presentation: VoiceComposerPresentation,
)

/**
 * Binds a [VoiceInputController] to a [TextFieldState].
 *
 * The field is the source of truth for text and caret: recording snapshots it,
 * and the confirmed transcript commits through an edit so the caret lands at
 * the end of the inserted text. [revision] bumps on every text change the
 * field reports, which is how a mid-recording keystroke makes the transcript
 * stale instead of silently overwriting it.
 */
@Composable
fun rememberDictation(ownerKey: Any?, field: TextFieldState): Dictation? {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var revision by remember(ownerKey) { mutableLongStateOf(0L) }
    var pendingPermission by remember(ownerKey) {
        mutableStateOf<CompletableDeferred<VoicePermissionResult>?>(null)
    }
    val permissionLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            pendingPermission?.complete(VoicePermissionResult(granted, canAskAgain = true))
            pendingPermission = null
        }

    val controller =
        remember(ownerKey) {
            // A device without a speech service gets no controller at all, which
            // the callers read as "no mic".
            if (!speechRecognitionAvailable(context)) {
                null
            } else {
                VoiceInputController(
                    scope = scope,
                    engineFactory = { listener ->
                        SpeechRecognizerVoiceEngine.create(context, listener)
                    },
                    requestPermission = {
                        if (microphoneAllowed(context)) {
                            VoicePermissionResult(granted = true, canAskAgain = true)
                        } else {
                            val deferred = CompletableDeferred<VoicePermissionResult>()
                            // A second tap while a prompt is open reuses the new
                            // deferred; the stale one completes denied so its
                            // start() unwinds instead of hanging forever.
                            pendingPermission?.complete(
                                VoicePermissionResult(granted = false, canAskAgain = true)
                            )
                            pendingPermission = deferred
                            permissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
                            deferred.await()
                        }
                    },
                    readDraft = {
                        VoiceDraftSnapshot(
                            ownerKey = ownerKey.toString(),
                            text = field.text.toString(),
                            selectionStart = field.selection.start,
                            selectionEnd = field.selection.end,
                            revision = revision,
                        )
                    },
                    commitDraft = { text, cursor ->
                        field.edit {
                            replace(0, length, text)
                            selection = TextRange(cursor)
                        }
                    },
                )
            }
        }

    // The draft snapshot's staleness revision follows every edit the field
    // reports: typing, a commit, or an external restore all count.
    LaunchedEffect(controller, field) {
        if (controller == null) return@LaunchedEffect
        snapshotFlow { field.text.toString() }.collect { revision += 1 }
    }
    DisposableEffect(controller) {
        onDispose {
            controller?.dispose()
            pendingPermission?.complete(
                VoicePermissionResult(granted = false, canAskAgain = false)
            )
        }
    }
    if (controller == null) return null

    val state by controller.state.collectAsStateWithLifecycle()
    return Dictation(controller, state, resolveVoiceComposerPresentation(state))
}

private fun microphoneAllowed(context: Context): Boolean =
    ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
        PackageManager.PERMISSION_GRANTED

@Suppress("DEPRECATION")
private fun speechRecognitionAvailable(context: Context): Boolean =
    android.speech.SpeechRecognizer.isRecognitionAvailable(context)

/**
 * The mic control: starts dictation, or opens this app's system settings when
 * the permission was denied for good. Sits in the collapsed pill's trailing
 * slot and in the expanded toolbar while idle — matching the RN dictation
 * control.
 */
@Composable
fun DictationMicControl(dictation: Dictation?, modifier: Modifier = Modifier) {
    if (dictation == null) return
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val openSettings = dictation.state.errorAction == VoiceInputErrorAction.Settings
    S5ComposerControl(
        label = null,
        icon = Icons.Rounded.Mic,
        contentDescription =
            if (openSettings) "Open microphone settings" else "Start dictation",
        onClick = {
            if (openSettings) {
                dictation.controller.dismissError()
                runCatching {
                    context.startActivity(
                        Intent(
                            Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                            Uri.fromParts("package", context.packageName, null),
                        )
                    )
                }
            } else {
                scope.launch { dictation.controller.start() }
            }
        },
        modifier = modifier,
    )
}

/**
 * The toolbar the composer swaps in while a session is active (or showing its
 * error): cancel on the left, the live status in the middle, confirm on the
 * right — the RN dictation row.
 */
@Composable
fun DictationToolbar(dictation: Dictation, modifier: Modifier = Modifier) {
    val state = dictation.state
    val presentation = dictation.presentation
    S5ComposerToolbarRow(modifier) {
        if (presentation.showsCancel) {
            S5ComposerControl(
                label = null,
                icon = Icons.Rounded.Close,
                contentDescription = "Cancel dictation",
                onClick = { dictation.controller.cancel() },
            )
        }
        Row(
            Modifier.weight(1f).padding(horizontal = S5Theme.spacing.small),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            when {
                presentation.statusIsError -> {
                    Text(
                        presentation.statusLabel.orEmpty(),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    S5ComposerControl(
                        label = null,
                        icon = Icons.Rounded.Close,
                        contentDescription = "Dismiss voice input error",
                        onClick = { dictation.controller.dismissError() },
                    )
                }
                // While the recognizer streams partials, the latest one is the
                // most honest status; between partials the elapsed clock shows.
                state.phase == VoiceInputPhase.Recording && state.partial != null -> {
                    Text(
                        state.partial,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                }
                else -> {
                    Text(
                        presentation.statusLabel.orEmpty(),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
        if (presentation.trailingAction == VoiceComposerPresentation.TrailingAction.Confirm) {
            if (presentation.confirmationEnabled) {
                S5ComposerAction(
                    icon = Icons.Rounded.Check,
                    label = "Finish dictation",
                    onClick = { dictation.controller.confirm() },
                )
            } else {
                S5InlineLoading(Modifier.padding(S5Theme.spacing.tiny))
            }
        } else {
            DictationMicControl(dictation)
        }
    }
}
