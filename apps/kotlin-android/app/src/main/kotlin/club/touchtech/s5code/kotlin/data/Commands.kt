package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ComposerAttachment
import club.touchtech.s5code.kotlin.model.ProviderOptionSelection
import club.touchtech.s5code.kotlin.model.ProviderOptionValue
import club.touchtech.s5code.kotlin.model.RuntimeMode
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.UserInputAnswer
import java.util.UUID
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.add
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Builders for `orchestration.dispatchCommand` payloads, matching
 * `ClientOrchestrationCommand` in `packages/contracts/src/orchestration.ts`.
 *
 * Commands are built as JSON rather than typed classes because the union has 23
 * members with almost no shared fields, and each one is written in exactly one
 * place. A serializable class per command would be 23 classes to express what is
 * already a one-line literal.
 *
 * Every command carries a client-generated `commandId`. The server uses it for
 * idempotency, so a retry after a dropped socket must reuse the same id or the
 * turn starts twice.
 */
object Commands {

    fun newCommandId(): String = UUID.randomUUID().toString()

    private fun now(): String = java.time.Instant.now().toString()

    /**
     * A `modelSelection` block. Options ride along in the contract's canonical
     * array shape and are omitted when empty rather than sent as `[]`, which the
     * server would read as "clear every option" instead of "nothing to say".
     */
    private fun JsonObjectBuilder.putModelSelection(
        instanceId: String,
        model: String,
        options: List<ProviderOptionSelection>,
    ) = putJsonObject("modelSelection") {
        put("instanceId", instanceId)
        put("model", model)
        if (options.isNotEmpty()) {
            putJsonArray("options") {
                options.forEach { selection ->
                    addJsonObject {
                        put("id", selection.id)
                        when (val value = selection.value) {
                            is ProviderOptionValue.Text -> put("value", value.value)
                            is ProviderOptionValue.Flag -> put("value", value.value)
                        }
                    }
                }
            }
        }
    }

    /**
     * Starts a turn on an existing thread. Settings are explicit because RN stages
     * model/runtime changes in the composer and applies the same snapshot to both
     * thread metadata and the turn command when Send is tapped.
     */
    fun startTurn(
        threadId: String,
        text: String,
        attachments: List<ComposerAttachment>,
        attachmentDataUrls: Map<String, String>,
        settings: ThreadSettings,
        commandId: String = newCommandId(),
        messageId: String = UUID.randomUUID().toString(),
        createdAt: String = now(),
    ): JsonObject = buildJsonObject {
        put("type", "thread.turn.start")
        put("commandId", commandId)
        put("threadId", threadId)
        putJsonObject("message") {
            put("messageId", messageId)
            put("role", "user")
            put("text", text)
            putJsonArray("attachments") {
                attachments.forEach { attachment ->
                    val dataUrl = attachmentDataUrls[attachment.id] ?: return@forEach
                    addJsonObject {
                        put("type", "image")
                        put("name", attachment.name)
                        put("mimeType", attachment.mimeType)
                        put("sizeBytes", attachment.sizeBytes)
                        put("dataUrl", dataUrl)
                    }
                }
            }
        }
        putModelSelection(settings.provider.instanceId, settings.model, settings.options)
        put("runtimeMode", settings.approvalPolicy.toRuntimeMode())
        put("interactionMode", if (settings.runtimeMode == RuntimeMode.Plan) "plan" else "default")
        put("createdAt", createdAt)
    }

    /**
     * Creates a thread and starts its first turn in one command, mirroring
     * `buildProjectThreadStartTurnInput` in the RN client. The bootstrap block is
     * what makes this atomic: a failure to prepare the worktree fails the whole
     * command instead of leaving an empty thread in the list.
     */
    fun startTurnBootstrapping(
        threadId: String,
        projectId: String,
        projectCwd: String,
        title: String,
        text: String,
        attachments: List<ComposerAttachment>,
        attachmentDataUrls: Map<String, String>,
        instanceId: String,
        model: String,
        options: List<ProviderOptionSelection>,
        runtimeMode: String,
        interactionMode: String,
        branch: String?,
        newWorktree: Boolean,
        commandId: String = newCommandId(),
        messageId: String = UUID.randomUUID().toString(),
        createdAt: String = now(),
    ): JsonObject = buildJsonObject {
        put("type", "thread.turn.start")
        put("commandId", commandId)
        put("threadId", threadId)
        putJsonObject("message") {
            put("messageId", messageId)
            put("role", "user")
            put("text", text)
            putJsonArray("attachments") {
                attachments.forEach { attachment ->
                    val dataUrl = attachmentDataUrls[attachment.id] ?: return@forEach
                    addJsonObject {
                        put("type", "image")
                        put("name", attachment.name)
                        put("mimeType", attachment.mimeType)
                        put("sizeBytes", attachment.sizeBytes)
                        put("dataUrl", dataUrl)
                    }
                }
            }
        }
        putModelSelection(instanceId, model, options)
        put("titleSeed", title)
        put("runtimeMode", runtimeMode)
        put("interactionMode", interactionMode)
        putJsonObject("bootstrap") {
            putJsonObject("createThread") {
                put("projectId", projectId)
                put("title", title)
                putModelSelection(instanceId, model, options)
                put("runtimeMode", runtimeMode)
                put("interactionMode", interactionMode)
                if (branch != null) put("branch", branch) else put("branch", null as String?)
                // In worktree mode the path is decided by prepareWorktree, so
                // sending one here would fight it.
                put("worktreePath", null as String?)
                put("createdAt", createdAt)
            }
            if (newWorktree && branch != null) {
                putJsonObject("prepareWorktree") {
                    put("projectCwd", projectCwd)
                    put("baseBranch", branch)
                    put("branch", worktreeBranchName(title))
                }
                // A fresh worktree has no node_modules; skipping setup would give
                // the agent a workspace where nothing builds.
                put("runSetupScript", true)
            }
        }
        put("createdAt", createdAt)
    }

    fun interruptTurn(threadId: String, turnId: String?): JsonObject = buildJsonObject {
        put("type", "thread.turn.interrupt")
        put("commandId", newCommandId())
        put("threadId", threadId)
        if (turnId != null) put("turnId", turnId)
        put("createdAt", now())
    }

    fun respondToApproval(threadId: String, requestId: String, decision: String): JsonObject =
        buildJsonObject {
            put("type", "thread.approval.respond")
            put("commandId", newCommandId())
            put("threadId", threadId)
            put("requestId", requestId)
            put("decision", decision)
            put("createdAt", now())
        }

    /**
     * Answers a structured input request. Answers are keyed by question id, and
     * the value is a string for single answers or an array for multi-select —
     * `ProviderUserInputAnswers` is an open record, so the shape has to match what
     * the provider asked for.
     */
    fun respondToUserInput(
        threadId: String,
        requestId: String,
        answers: Map<String, UserInputAnswer>,
    ): JsonObject = buildJsonObject {
        put("type", "thread.user-input.respond")
        put("commandId", newCommandId())
        put("threadId", threadId)
        put("requestId", requestId)
        putJsonObject("answers") {
            answers.forEach { (questionId, answer) ->
                when (answer) {
                    is UserInputAnswer.Text -> put(questionId, answer.value)
                    is UserInputAnswer.Choices ->
                        putJsonArray(questionId) { answer.values.forEach { add(it) } }
                }
            }
        }
        put("createdAt", now())
    }

    fun updateMeta(
        threadId: String,
        title: String? = null,
        regenerateTitle: Boolean = false,
        instanceId: String? = null,
        model: String? = null,
        options: List<ProviderOptionSelection> = emptyList(),
        commandId: String = newCommandId(),
    ): JsonObject = buildJsonObject {
        put("type", "thread.meta.update")
        put("commandId", commandId)
        put("threadId", threadId)
        // The contract rejects both together, so the caller picks one.
        if (regenerateTitle) put("regenerateTitle", true) else if (title != null) put("title", title)
        if (instanceId != null && model != null) {
            putModelSelection(instanceId, model, options)
        }
    }

    fun setRuntimeMode(
        threadId: String,
        runtimeMode: String,
        commandId: String = newCommandId(),
        createdAt: String = now(),
    ): JsonObject = buildJsonObject {
        put("type", "thread.runtime-mode.set")
        put("commandId", commandId)
        put("threadId", threadId)
        put("runtimeMode", runtimeMode)
        put("createdAt", createdAt)
    }

    fun setInteractionMode(
        threadId: String,
        interactionMode: String,
        commandId: String = newCommandId(),
        createdAt: String = now(),
    ): JsonObject = buildJsonObject {
        put("type", "thread.interaction-mode.set")
        put("commandId", commandId)
        put("threadId", threadId)
        put("interactionMode", interactionMode)
        put("createdAt", createdAt)
    }

    fun lifecycle(type: String, threadId: String): JsonObject = buildJsonObject {
        put("type", type)
        put("commandId", newCommandId())
        put("threadId", threadId)
    }

    /** Unsettle and unsnooze only accept "user": activity resets are server-side. */
    fun lifecycleByUser(type: String, threadId: String): JsonObject = buildJsonObject {
        put("type", type)
        put("commandId", newCommandId())
        put("threadId", threadId)
        put("reason", "user")
    }

    fun snooze(threadId: String, untilIso: String): JsonObject = buildJsonObject {
        put("type", "thread.snooze")
        put("commandId", newCommandId())
        put("threadId", threadId)
        put("snoozedUntil", untilIso)
    }

    fun revertCheckpoint(threadId: String, turnCount: Int): JsonObject = buildJsonObject {
        put("type", "thread.checkpoint.revert")
        put("commandId", newCommandId())
        put("threadId", threadId)
        put("turnCount", turnCount)
        put("createdAt", now())
    }

    /**
     * Registers a project. `projectId` is client-generated because the command is
     * idempotent on it: a retry after a dropped socket must not create a second
     * project for the same directory.
     */
    fun createProject(
        projectId: String,
        title: String,
        workspaceRoot: String,
        createWorkspaceRootIfMissing: Boolean,
    ): JsonObject = buildJsonObject {
        put("type", "project.create")
        put("commandId", newCommandId())
        put("projectId", projectId)
        put("title", title)
        put("workspaceRoot", workspaceRoot)
        put("createWorkspaceRootIfMissing", createWorkspaceRootIfMissing)
        put("createdAt", now())
    }

    /**
     * Worktree branch name for a new task. Prefixed and slugified so a branch
     * created from a phone is recognisable in a terminal later.
     */
    private fun worktreeBranchName(title: String): String {
        val slug =
            title
                .lowercase()
                .replace(Regex("[^a-z0-9]+"), "-")
                .trim('-')
                .take(32)
                .ifBlank { "task" }
        return "s5/$slug-${UUID.randomUUID().toString().take(6)}"
    }
}

/** Default snooze: tomorrow morning, matching the RN preset the row menu uses. */
fun tomorrowMorningIso(): String {
    val zone = java.time.ZoneId.systemDefault()
    val tomorrow = java.time.LocalDate.now(zone).plusDays(1).atTime(9, 0)
    return tomorrow.atZone(zone).toInstant().toString()
}
