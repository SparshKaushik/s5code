package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.Remove
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.data.SECRET_REQUEST_DEFAULT_PLACEHOLDER
import club.touchtech.s5code.kotlin.data.SECRET_REQUEST_PRIVACY_NOTE
import club.touchtech.s5code.kotlin.data.SecretRequestAnswer
import club.touchtech.s5code.kotlin.data.SecretRequestDisplay
import club.touchtech.s5code.kotlin.data.secretRequestDisplay
import club.touchtech.s5code.kotlin.data.secretRequestFailureMessage
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5Card
import club.touchtech.s5code.kotlin.design.component.S5CardTone
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.FeedEntry
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/**
 * A `secret_request` row — the port of `SecretRequestCard.tsx`.
 *
 * Pending and local (the request was raised on this thread), it is the card:
 * what is asked, why, a masked field, Save/Decline, and the privacy note. The
 * typed value lives only in this composable's state and the RPC payload —
 * `secrets.answerRequest` — never in the transcript, a log, or an error.
 * Every other status (or an inherited request a fork shows but the original
 * thread answers) renders as the one-line outcome.
 */
@Composable
internal fun SecretRequestRow(
    entry: FeedEntry.SecretRequest,
    modifier: Modifier = Modifier,
    /**
     * Sends `secrets.answerRequest`; throws on failure so the card can show
     * [secretRequestFailureMessage]. Suspend because the answer is an RPC.
     */
    onAnswer: suspend (SecretRequestAnswer) -> Unit = {},
) {
    when (val display = secretRequestDisplay(entry)) {
        SecretRequestDisplay.Pending ->
            PendingSecretRequestCard(entry, onAnswer, modifier)
        else ->
            SecretRequestLine(
                entry = entry,
                display = display,
                icon =
                    when {
                        display is SecretRequestDisplay.PendingElsewhere -> Icons.Rounded.Lock
                        (display as SecretRequestDisplay.Answered).outcome == "saved" ->
                            Icons.Rounded.Check
                        else -> Icons.Rounded.Remove
                    },
                modifier = modifier,
            )
    }
}

/** The answered or inherited-request form: icon plus one line of outcome text. */
@Composable
private fun SecretRequestLine(
    entry: FeedEntry.SecretRequest,
    display: SecretRequestDisplay,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    modifier: Modifier,
) {
    val line =
        when (display) {
            SecretRequestDisplay.Pending ->
                "${entry.label} · Waiting for your answer"
            is SecretRequestDisplay.PendingElsewhere ->
                "${entry.label} · ${display.label}"
            is SecretRequestDisplay.Answered ->
                "${entry.label} · ${display.label}"
        }
    Row(
        modifier
            .fillMaxWidth()
            .padding(vertical = S5Theme.spacing.tiny, horizontal = S5Theme.spacing.tiny),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
    ) {
        Icon(
            icon,
            contentDescription = null,
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            line,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/**
 * The interactive form. Same hierarchy as the other clients: label, reason,
 * field, then the promise about where the value goes. The field clears once
 * the answer is sent; the card flips to its answered row when the item updates.
 */
@Composable
private fun PendingSecretRequestCard(
    entry: FeedEntry.SecretRequest,
    onAnswer: suspend (SecretRequestAnswer) -> Unit,
    modifier: Modifier,
) {
    val scope = rememberCoroutineScope()
    var secret by remember(entry.id) { mutableStateOf("") }
    var submitting by remember(entry.id) { mutableStateOf(false) }
    var failure by remember(entry.id) { mutableStateOf<String?>(null) }

    fun send(answer: SecretRequestAnswer) {
        // The server rejects a blank save; the disabled button is the guard,
        // this is the belt. The in-flight check is synchronous in spirit: two
        // taps between frames must not double-send.
        if (submitting) return
        if (answer is SecretRequestAnswer.Save && answer.secret.trim().isEmpty()) return
        submitting = true
        failure = null
        scope.launch {
            try {
                onAnswer(answer)
                secret = ""
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                // The failure's payload may hold the request; only the mapped
                // message is ever shown, never the typed value.
                failure = secretRequestFailureMessage(error)
            } finally {
                submitting = false
            }
        }
    }

    S5Card(tone = S5CardTone.Standard, modifier = modifier.fillMaxWidth()) {
        Column(
            Modifier.padding(S5Theme.spacing.large),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny)) {
                Text(entry.label, style = MaterialTheme.typography.titleSmallEmphasized)
                if (entry.reason.isNotBlank()) {
                    Text(
                        entry.reason,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            // The app's own S5TextField has no masking, so the field is the
            // Material one — the credential flow in settings does the same.
            OutlinedTextField(
                value = secret,
                onValueChange = { secret = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = {
                    Text(entry.placeholder ?: SECRET_REQUEST_DEFAULT_PLACEHOLDER)
                },
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions =
                    KeyboardOptions(
                        keyboardType = KeyboardType.Password,
                        imeAction = ImeAction.Done,
                        autoCorrectEnabled = false,
                    ),
                enabled = !submitting,
                singleLine = true,
            )
            failure?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            S5Button(
                text = "Save securely",
                onClick = { send(SecretRequestAnswer.Save(secret)) },
                enabled = !submitting && secret.isNotBlank(),
                emphasis = S5ActionEmphasis.Primary,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
            ) {
                Icon(
                    Icons.Rounded.Lock,
                    contentDescription = null,
                    modifier = Modifier.size(13.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    SECRET_REQUEST_PRIVACY_NOTE,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                // Quiet like the web card's decline: the field and Save are
                // the action.
                TextButton(
                    onClick = { send(SecretRequestAnswer.Decline) },
                    enabled = !submitting,
                ) {
                    Text("Decline", style = MaterialTheme.typography.labelMedium)
                }
            }
        }
    }
}
