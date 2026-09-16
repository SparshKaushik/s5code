package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5CodeBlock
import club.touchtech.s5code.kotlin.design.component.S5ComposerField
import club.touchtech.s5code.kotlin.design.component.S5ComposerSurface
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.rememberDraftTextFieldState
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.ApprovalKind
import club.touchtech.s5code.kotlin.model.ApprovalOption
import club.touchtech.s5code.kotlin.model.PendingApproval
import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import club.touchtech.s5code.kotlin.model.UserInputKind
import club.touchtech.s5code.kotlin.model.UserInputQuestion

/**
 * Approval gate.
 *
 * Compact on purpose: this card lands in a transcript you are already reading,
 * directly above the composer, so it competes with the message you are trying to
 * see. Prominence comes from the accent rail and the tonal container, not from
 * 16dp padding and full-width hero buttons. The old version was tall enough to
 * push the command it was asking about off screen, which is the one thing you
 * need to read before deciding.
 */
@Composable
fun ApprovalCard(
    approval: PendingApproval,
    onDecision: (String) -> Unit,
    modifier: Modifier = Modifier,
    submitting: Boolean = false,
) {
    // The provider names its own decisions; requests without options (older
    // payloads, Claude's permission prompt) fall back to the same generic set
    // the RN card uses, in approve/session/deny order.
    val options = approval.options.ifEmpty { DEFAULT_APPROVAL_OPTIONS }
    GateCard(
        accent = S5Theme.status.approval,
        container = S5Theme.status.approvalContainer,
        onContainer = S5Theme.status.onApprovalContainer,
        icon = Icons.Rounded.CheckCircle,
        label =
            when (approval.kind) {
                ApprovalKind.Command -> "Approve command"
                ApprovalKind.FileWrite -> "Approve file write"
                ApprovalKind.NetworkAccess -> "Approve network access"
            },
        modifier = modifier,
    ) {
        Text(approval.title, style = MaterialTheme.typography.titleSmallEmphasized)
        Text(
            approval.detail,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (approval.command != null) {
            S5CodeBlock(lines = listOf(approval.command), language = "bash")
        }
        // A provider caution, such as a prompt-injection warning on an "allow"
        // option, belongs next to the actions that carry it.
        options.firstNotNullOfOrNull { it.warning }?.let { warning ->
            Text(
                warning,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
        // Provider labels do not fit one line on a small phone, so the actions
        // wrap rather than clip.
        FlowRow(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        ) {
            options.forEach { option ->
                S5Button(
                    text = option.label,
                    onClick = { onDecision(option.decision) },
                    emphasis = S5ActionEmphasis.Prominent,
                    // Accept is the hero, decline/cancel read as refusal, and
                    // every other decision (session, always) stays quiet — the
                    // same emphasis ladder the web card draws.
                    style =
                        when (option.decision) {
                            "accept" -> S5ButtonStyle.Filled
                            "decline", "cancel" -> S5ButtonStyle.Outlined
                            else -> S5ButtonStyle.Text
                        },
                    icon =
                        when (option.decision) {
                            "accept" -> Icons.Rounded.CheckCircle
                            "decline", "cancel" -> Icons.Rounded.Block
                            else -> null
                        },
                    enabled = !submitting,
                )
            }
        }
    }
}

/**
 * Buttons for a request that arrived without provider options. These are the
 * decisions every adapter understands, in the same order the RN card falls
 * back to.
 */
private val DEFAULT_APPROVAL_OPTIONS =
    listOf(
        ApprovalOption(decision = "accept", label = "Allow once"),
        ApprovalOption(decision = "acceptForSession", label = "Allow session"),
        ApprovalOption(decision = "decline", label = "Decline"),
    )

/** Structured input: every question is answered and submitted as one record. */
@Composable
fun UserInputCard(
    request: PendingUserInput,
    onSubmit: (Map<String, UserInputAnswer>) -> Unit,
    modifier: Modifier = Modifier,
    submitting: Boolean = false,
) {
    var textAnswers by remember(request.id) { mutableStateOf(emptyMap<String, String>()) }
    var selectedAnswers by
        remember(request.id) { mutableStateOf(emptyMap<String, Set<String>>()) }
    val answers = remember(request, textAnswers, selectedAnswers) {
        buildUserInputAnswers(request, textAnswers, selectedAnswers)
    }

    GateCard(
        accent = S5Theme.status.input,
        container = S5Theme.status.inputContainer,
        onContainer = S5Theme.status.onInputContainer,
        icon = Icons.AutoMirrored.Rounded.Send,
        label =
            if (request.questions.size == 1) "The agent has a question"
            else "The agent has ${request.questions.size} questions",
        modifier = modifier,
    ) {
        request.questions.forEach { question ->
            UserInputQuestionFields(
                requestId = request.id,
                question = question,
                customAnswer = textAnswers[question.id].orEmpty(),
                selected = selectedAnswers[question.id].orEmpty(),
                submitting = submitting,
                onCustomAnswer = { value ->
                    textAnswers = textAnswers + (question.id to value)
                    if (value.isNotBlank()) selectedAnswers = selectedAnswers - question.id
                },
                onSelected = { values ->
                    selectedAnswers = selectedAnswers + (question.id to values)
                    if (values.isNotEmpty()) textAnswers = textAnswers - question.id
                },
            )
        }
        S5Button(
            text = if (submitting) "Sending…" else "Submit answers",
            onClick = { answers?.let(onSubmit) },
            emphasis = S5ActionEmphasis.Prominent,
            icon = Icons.AutoMirrored.Rounded.Send,
            enabled = answers != null && !submitting,
        )
    }
}

@Composable
private fun UserInputQuestionFields(
    requestId: String,
    question: UserInputQuestion,
    customAnswer: String,
    selected: Set<String>,
    submitting: Boolean,
    onCustomAnswer: (String) -> Unit,
    onSelected: (Set<String>) -> Unit,
) {
    val answerState =
        rememberDraftTextFieldState("$requestId/${question.id}", customAnswer, onCustomAnswer)
    Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
        Text(
            question.header,
            style = MaterialTheme.typography.labelMediumEmphasized,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(question.prompt, style = MaterialTheme.typography.titleSmallEmphasized)
        if (question.options.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                question.options.forEachIndexed { index, option ->
                    S5SelectableRow(
                        label = option,
                        selected = option in selected,
                        onClick = {
                            if (submitting) return@S5SelectableRow
                            onSelected(
                                when {
                                    question.kind == UserInputKind.SingleSelect -> setOf(option)
                                    option in selected -> selected - option
                                    else -> selected + option
                                }
                            )
                        },
                        position = rowPosition(index, question.options.size),
                    )
                }
            }
        }
        // Providers accept a free-form value even when choices are advertised;
        // this is the RN card's “Or type a custom answer” escape hatch.
        S5ComposerSurface(cornerRadius = 20.dp) {
            Row(
                Modifier.padding(
                    start = S5Theme.spacing.medium,
                    end = S5Theme.spacing.medium,
                    top = S5Theme.spacing.tiny,
                    bottom = S5Theme.spacing.tiny,
                ),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                S5ComposerField(
                    state = answerState,
                    placeholder =
                        if (question.options.isEmpty()) "Your answer"
                        else "Or type a custom answer",
                    maxLines = 4,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

/** Resolves the request-wide answer record; null means at least one answer is missing. */
internal fun buildUserInputAnswers(
    request: PendingUserInput,
    textAnswers: Map<String, String>,
    selectedAnswers: Map<String, Set<String>>,
): Map<String, UserInputAnswer>? {
    val answers = linkedMapOf<String, UserInputAnswer>()
    request.questions.forEach { question ->
        val custom = textAnswers[question.id]?.trim().orEmpty()
        if (custom.isNotEmpty()) {
            answers[question.id] = UserInputAnswer.Text(custom)
            return@forEach
        }
        val selected = question.options.filter { it in selectedAnswers[question.id].orEmpty() }
        when (question.kind) {
            UserInputKind.MultiSelect -> {
                if (selected.isEmpty()) return null
                answers[question.id] = UserInputAnswer.Choices(selected)
            }
            UserInputKind.SingleSelect -> {
                val value = selected.firstOrNull() ?: return null
                answers[question.id] = UserInputAnswer.Text(value)
            }
            UserInputKind.Text -> return null
        }
    }
    return answers
}

/**
 * Shared frame for the two gates: a tonal container with a status-colored rail
 * and a small icon-plus-label header. The rail is what lets a compact card read
 * as urgent without a large icon badge or extra padding.
 */
@Composable
private fun GateCard(
    accent: Color,
    container: Color,
    onContainer: Color,
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        color = container,
        contentColor = onContainer,
    ) {
        // IntrinsicSize.Min lets the rail match the content's height without a
        // measured value or a fixed one.
        Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.fillMaxHeight().width(3.dp).background(accent))
            Column(
                Modifier.padding(S5Theme.spacing.medium),
                verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    Icon(icon, contentDescription = null, modifier = Modifier.size(14.dp), tint = accent)
                    Text(label, style = MaterialTheme.typography.labelMediumEmphasized)
                }
                content()
            }
        }
    }
}
