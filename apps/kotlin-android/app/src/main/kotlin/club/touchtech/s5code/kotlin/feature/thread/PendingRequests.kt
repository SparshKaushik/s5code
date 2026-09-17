package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.Image
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.InsertDriveFile
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.AttachFile
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Image
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.design.component.S5ActionEmphasis
import club.touchtech.s5code.kotlin.design.component.S5Button
import club.touchtech.s5code.kotlin.design.component.S5ButtonStyle
import club.touchtech.s5code.kotlin.design.component.S5CodeBlock
import club.touchtech.s5code.kotlin.design.component.S5ComposerField
import club.touchtech.s5code.kotlin.design.component.S5ComposerSurface
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5InlineLoading
import club.touchtech.s5code.kotlin.design.component.S5SelectableRow
import club.touchtech.s5code.kotlin.design.component.rememberDraftTextFieldState
import club.touchtech.s5code.kotlin.design.component.rememberImageThumbnail
import club.touchtech.s5code.kotlin.design.component.rowPosition
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.files.attachmentSizeLabel
import club.touchtech.s5code.kotlin.model.ApprovalKind
import club.touchtech.s5code.kotlin.model.ApprovalOption
import club.touchtech.s5code.kotlin.model.PendingApproval
import club.touchtech.s5code.kotlin.model.PendingUserInput
import club.touchtech.s5code.kotlin.model.QuestionAttachment
import club.touchtech.s5code.kotlin.model.QuestionAttachmentKind
import club.touchtech.s5code.kotlin.model.QuestionAttachmentStatus
import club.touchtech.s5code.kotlin.model.SentAttachment
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
                ApprovalKind.FileRead -> "Approve file read"
                ApprovalKind.FileWrite -> "Approve file write"
                ApprovalKind.McpElicitation -> "Answer MCP request"
                ApprovalKind.NetworkAccess -> "Approve network access"
            },
        modifier = modifier,
    ) {
        // The provider's own name for the request beats our kind label, matching
        // `appName ?? requestKind` in the RN card.
        Text(
            approval.appName ?: approval.title,
            style = MaterialTheme.typography.titleSmallEmphasized,
        )
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

/**
 * What a pending user-input request accepts as attachments, from the
 * environment's advertised capabilities. Null on surfaces that cannot stage
 * uploads at all (the home card has no thread detail yet), which hides the
 * pickers the same way an incapable server does.
 */
data class QuestionAttachmentSupport(
    /** `capabilities.questionAttachments` — the feature gate. */
    val enabled: Boolean,
    /** `capabilities.fileAttachments != null` — non-image uploads allowed. */
    val supportsFiles: Boolean,
)

/** The card's full submission: resolved answers plus uploaded attachments per question. */
data class UserInputSubmission(
    val answers: Map<String, UserInputAnswer>,
    val attachmentsByQuestionId: Map<String, List<SentAttachment>>,
)

/** Structured input: every question is answered and submitted as one record. */
@Composable
fun UserInputCard(
    request: PendingUserInput,
    onSubmit: (UserInputSubmission) -> Unit,
    modifier: Modifier = Modifier,
    submitting: Boolean = false,
    /** Closes a dismissible request without answering. Null when not allowed. */
    onDismiss: (() -> Unit)? = null,
    /**
     * Attachments staged per question id, uploads already running against the
     * server. Empty on surfaces that do not stage uploads.
     */
    attachments: Map<String, List<QuestionAttachment>> = emptyMap(),
    attachmentSupport: QuestionAttachmentSupport? = null,
    /** Questions whose picker is still materializing files; submit waits on them. */
    preparingQuestions: Set<String> = emptySet(),
    onPickImages: (String) -> Unit = {},
    onPickFiles: (String) -> Unit = {},
    onRemoveAttachment: (String, QuestionAttachment) -> Unit = { _, _ -> },
    onRetryAttachment: (String, QuestionAttachment) -> Unit = { _, _ -> },
) {
    var textAnswers by remember(request.id) { mutableStateOf(emptyMap<String, String>()) }
    var selectedAnswers by
        remember(request.id) { mutableStateOf(emptyMap<String, Set<String>>()) }
    val submission =
        remember(request, textAnswers, selectedAnswers, attachments, preparingQuestions) {
            buildUserInputSubmission(request, textAnswers, selectedAnswers, attachments, preparingQuestions)
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
                attachments = attachments[question.id].orEmpty(),
                attachmentSupport = attachmentSupport,
                onPickImages = { onPickImages(question.id) },
                onPickFiles = { onPickFiles(question.id) },
                onRemoveAttachment = { onRemoveAttachment(question.id, it) },
                onRetryAttachment = { onRetryAttachment(question.id, it) },
            )
        }
        S5Button(
            text = if (submitting) "Sending…" else "Submit answers",
            onClick = { submission?.let(onSubmit) },
            emphasis = S5ActionEmphasis.Prominent,
            icon = Icons.AutoMirrored.Rounded.Send,
            enabled = submission != null && !submitting,
        )
        if (onDismiss != null) {
            S5Button(
                text = "Dismiss without answering",
                onClick = onDismiss,
                emphasis = S5ActionEmphasis.Secondary,
                style = S5ButtonStyle.Text,
                enabled = !submitting,
            )
        }
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
    attachments: List<QuestionAttachment>,
    attachmentSupport: QuestionAttachmentSupport?,
    onPickImages: () -> Unit,
    onPickFiles: () -> Unit,
    onRemoveAttachment: (QuestionAttachment) -> Unit,
    onRetryAttachment: (QuestionAttachment) -> Unit,
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
                        label = option.label,
                        supporting =
                            // A description identical to the label is noise;
                            // providers emit one anyway.
                            option.description?.takeIf { it != option.label },
                        selected = option.answerValue in selected,
                        onClick = {
                            if (submitting) return@S5SelectableRow
                            onSelected(
                                when {
                                    question.kind == UserInputKind.SingleSelect ->
                                        setOf(option.answerValue)
                                    option.answerValue in selected -> selected - option.answerValue
                                    else -> selected + option.answerValue
                                }
                            )
                        },
                        position = rowPosition(index, question.options.size),
                    )
                }
            }
        }
        // Attachments sit between the options and the custom answer, matching
        // RN's `QuestionAttachments`: the pickers only exist when the server
        // accepts question uploads, and a question that disallows free-form
        // answers takes no attachments either (it returns null there).
        if (question.allowCustomAnswer) {
            if (attachmentSupport?.enabled == true) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
                    verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
                ) {
                    S5Button(
                        text = "Add media",
                        onClick = onPickImages,
                        emphasis = S5ActionEmphasis.Secondary,
                        style = S5ButtonStyle.Outlined,
                        icon = Icons.Rounded.Image,
                        enabled = !submitting,
                    )
                    if (attachmentSupport.supportsFiles) {
                        S5Button(
                            text = "Add file",
                            onClick = onPickFiles,
                            emphasis = S5ActionEmphasis.Secondary,
                            style = S5ButtonStyle.Outlined,
                            icon = Icons.Rounded.AttachFile,
                            enabled = !submitting,
                        )
                    }
                }
            }
            QuestionAttachmentStrip(
                attachments = attachments,
                enabled = !submitting,
                onRemove = onRemoveAttachment,
                onRetry = onRetryAttachment,
            )
            // Providers accept a free-form value even when choices are advertised,
            // unless they explicitly disallow it — the RN card's “Or type a custom
            // answer” escape hatch.
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
}

/** Resolves the request-wide answer record; null means at least one answer is missing. */
internal fun buildUserInputAnswers(
    request: PendingUserInput,
    textAnswers: Map<String, String>,
    selectedAnswers: Map<String, Set<String>>,
): Map<String, UserInputAnswer>? {
    val answers = linkedMapOf<String, UserInputAnswer>()
    request.questions.forEach { question ->
        answers[question.id] =
            resolveUserInputAnswer(question, textAnswers, selectedAnswers, attachmentCount = 0)
                ?: return null
    }
    return answers
}

/**
 * Resolves the full submission, mirroring `resolvePendingUserInputAnswer` and
 * the `onSubmitUserInput` gate in the RN client:
 *
 * - A question still preparing files, or holding an attachment that is not
 *   [QuestionAttachmentStatus.Ready] yet, blocks the whole submission
 *   (`attachmentsBlocked`), no matter what else is answered.
 * - Ready attachments satisfy an unanswered question as an empty text answer —
 *   attachment-only answers are legal when the question takes custom input.
 * - Only uploaded attachments (non-null `uploadedAttachmentId`) enter
 *   `attachmentsByQuestionId`, keyed by the provider's question id.
 */
internal fun buildUserInputSubmission(
    request: PendingUserInput,
    textAnswers: Map<String, String>,
    selectedAnswers: Map<String, Set<String>>,
    attachments: Map<String, List<QuestionAttachment>>,
    preparingQuestions: Set<String> = emptySet(),
): UserInputSubmission? {
    val answers = linkedMapOf<String, UserInputAnswer>()
    val sent = linkedMapOf<String, List<SentAttachment>>()
    request.questions.forEach { question ->
        val staged = attachments[question.id].orEmpty()
        // RN's `attachmentsBlocked`: picking in flight, an unfinished upload,
        // or a failure all hold the submit rather than sending half the files.
        if (question.id in preparingQuestions || staged.any { it.status != QuestionAttachmentStatus.Ready }) {
            return null
        }
        answers[question.id] =
            resolveUserInputAnswer(question, textAnswers, selectedAnswers, staged.size)
                ?: return null
        val uploaded =
            staged.mapNotNull { attachment ->
                attachment.uploadedAttachmentId?.let { uploadId ->
                    SentAttachment(
                        id = uploadId,
                        name = attachment.name,
                        mimeType = attachment.mimeType,
                        sizeBytes = attachment.sizeBytes,
                        type = attachment.kind.wireValue,
                    )
                }
            }
        if (uploaded.isNotEmpty()) sent[question.id] = uploaded
    }
    return UserInputSubmission(answers, sent)
}

/**
 * One question's resolved answer, or null when it is still unanswered. The
 * precedence is the RN card's: a custom answer beats advertised options, and
 * staged attachments count as an answer only for questions that accept custom
 * input.
 */
private fun resolveUserInputAnswer(
    question: UserInputQuestion,
    textAnswers: Map<String, String>,
    selectedAnswers: Map<String, Set<String>>,
    attachmentCount: Int,
): UserInputAnswer? {
    val custom = textAnswers[question.id]?.trim().orEmpty()
    if (question.allowCustomAnswer && custom.isNotEmpty()) {
        return UserInputAnswer.Text(custom)
    }
    val selected =
        question.options
            .map { it.answerValue }
            .filter { it in selectedAnswers[question.id].orEmpty() }
    return when (question.kind) {
        UserInputKind.MultiSelect ->
            if (selected.isNotEmpty()) UserInputAnswer.Choices(selected)
            else attachmentOnlyAnswer(question, attachmentCount)
        UserInputKind.SingleSelect ->
            selected.firstOrNull()?.let(UserInputAnswer::Text)
                ?: attachmentOnlyAnswer(question, attachmentCount)
        UserInputKind.Text -> attachmentOnlyAnswer(question, attachmentCount)
    }
}

/** Files answer a question by themselves, as `""`, the same marker RN sends. */
private fun attachmentOnlyAnswer(
    question: UserInputQuestion,
    attachmentCount: Int,
): UserInputAnswer? =
    if (question.allowCustomAnswer && attachmentCount > 0) UserInputAnswer.Text("") else null

/**
 * The files staged on one question: a name chip per attachment carrying its
 * upload state, each with its own remove target. Tapping a failed chip retries
 * the upload — the RN strip's retry badge — while an in-flight one shows a
 * spinner instead of a false success.
 */
@Composable
private fun QuestionAttachmentStrip(
    attachments: List<QuestionAttachment>,
    enabled: Boolean,
    onRemove: (QuestionAttachment) -> Unit,
    onRetry: (QuestionAttachment) -> Unit,
) {
    if (attachments.isEmpty()) return
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.small),
        verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
    ) {
        attachments.forEach { attachment ->
            key(attachment.localId) {
                QuestionAttachmentChip(
                    attachment = attachment,
                    enabled = enabled,
                    onRemove = { onRemove(attachment) },
                    onRetry = { onRetry(attachment) },
                )
            }
        }
    }
}

@Composable
private fun QuestionAttachmentChip(
    attachment: QuestionAttachment,
    enabled: Boolean,
    onRemove: () -> Unit,
    onRetry: () -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
    ) {
        Row(
            Modifier.padding(start = S5Theme.spacing.small, end = S5Theme.spacing.tiny),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.tiny),
        ) {
            when (attachment.status) {
                QuestionAttachmentStatus.Uploading ->
                    S5InlineLoading(Modifier.size(14.dp))
                QuestionAttachmentStatus.Failed ->
                    Icon(
                        Icons.Rounded.ErrorOutline,
                        contentDescription = attachment.error ?: "Upload failed",
                        modifier = Modifier.size(14.dp),
                        tint = MaterialTheme.colorScheme.error,
                    )
                QuestionAttachmentStatus.Ready ->
                    QuestionAttachmentLeading(attachment)
            }
            Column(Modifier.padding(vertical = S5Theme.spacing.tiny).widthIn(max = 160.dp)) {
                Text(
                    attachment.name,
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    when (attachment.status) {
                        QuestionAttachmentStatus.Uploading -> "Uploading…"
                        QuestionAttachmentStatus.Failed -> attachment.error ?: "Upload failed"
                        QuestionAttachmentStatus.Ready -> attachmentSizeLabel(attachment.sizeBytes)
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color =
                        if (attachment.status == QuestionAttachmentStatus.Failed) {
                            MaterialTheme.colorScheme.error
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (attachment.status == QuestionAttachmentStatus.Failed) {
                S5IconButton(
                    icon = Icons.Rounded.Refresh,
                    label = "Retry uploading ${attachment.name}",
                    onClick = onRetry,
                    enabled = enabled,
                    compact = true,
                )
            }
            S5IconButton(
                icon = Icons.Rounded.Close,
                label = "Remove ${attachment.name}",
                onClick = onRemove,
                enabled = enabled,
                compact = true,
            )
        }
    }
}

/** A real thumbnail for staged images, a document glyph for everything else. */
@Composable
private fun QuestionAttachmentLeading(attachment: QuestionAttachment) {
    val thumbnail =
        if (attachment.kind == QuestionAttachmentKind.Image) {
            rememberImageThumbnail("file://${attachment.localPath}", 56)
        } else {
            null
        }
    if (thumbnail != null) {
        Image(
            bitmap = thumbnail,
            contentDescription = null,
            modifier = Modifier.size(28.dp).clip(RoundedCornerShape(6.dp)),
            contentScale = ContentScale.Crop,
        )
    } else {
        Icon(
            if (attachment.kind == QuestionAttachmentKind.Image) {
                Icons.Rounded.Image
            } else {
                Icons.AutoMirrored.Rounded.InsertDriveFile
            },
            contentDescription = null,
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
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
