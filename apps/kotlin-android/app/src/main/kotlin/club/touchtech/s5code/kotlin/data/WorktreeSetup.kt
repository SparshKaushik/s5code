package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.WorktreeSetupScript
import club.touchtech.s5code.kotlin.model.WorktreeSetupSnapshot
import club.touchtech.s5code.kotlin.model.WorktreeSetupStage
import club.touchtech.s5code.kotlin.transport.wire.ThreadActivityDto
import club.touchtech.s5code.kotlin.transport.wire.WorktreeSetupSnapshotDto
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject

/**
 * `worktreeSetup.ts` in `packages/client-runtime`: which setup snapshot the
 * timeline shows. The live stream wins while its sequence is at least the
 * recorded activity's; the recorded activity covers everything after the live
 * stream drops.
 *
 * The setup belongs to the thread's first turn: once the user has sent a
 * follow-up it is history and nothing about it is shown again, whatever its
 * outcome. Within that first turn a running setup always shows, a clean finish
 * leaves no trace once the turn is live (the setup is a means to the reply, not
 * part of the conversation), while a failed or cancelled one stays so the
 * outcome, exit code, and terminal are reachable. Before the turn is live
 * everything stays so nothing collapses in the handoff gap. Visibility never
 * depends on whether a turn happens to be running, which would make the card
 * come and go.
 */
const val WORKTREE_SETUP_ACTIVITY_KIND = "worktree-setup"

private val worktreeSetupJson = Json { ignoreUnknownKeys = true }

fun WorktreeSetupSnapshotDto.toModel(): WorktreeSetupSnapshot =
    WorktreeSetupSnapshot(
        threadId = threadId,
        phase = phase,
        startedAtMillis = startedAt?.let(::parseInstant),
        endedAtMillis = endedAt?.let(::parseInstant),
        branch = branch,
        baseRef = baseRef,
        worktreePath = worktreePath,
        setupScript =
            setupScript?.let {
                WorktreeSetupScript(name = it.name, command = it.command, terminalId = it.terminalId)
            },
        stages =
            stages.map { stage ->
                WorktreeSetupStage(
                    id = stage.id,
                    status = stage.status,
                    startedAtMillis = stage.startedAt?.let(::parseInstant),
                    endedAtMillis = stage.endedAt?.let(::parseInstant),
                    percent = stage.percent,
                    detail = stage.detail,
                    tail = stage.tail,
                )
            },
        error = error,
        sequence = sequence,
    )

/**
 * The newest `worktree-setup` activity on the thread, decoded — the bootstrap
 * writes under a fixed id once per outcome, so the last decode wins.
 */
fun findRecordedWorktreeSetup(
    activities: List<ThreadActivityDto>,
    threadId: String,
): WorktreeSetupSnapshot? {
    for (index in activities.indices.reversed()) {
        val activity = activities[index]
        if (activity.kind != WORKTREE_SETUP_ACTIVITY_KIND) continue
        val payload = activity.payload as? JsonObject ?: continue
        val snapshot =
            runCatching {
                    worktreeSetupJson.decodeFromJsonElement(
                        WorktreeSetupSnapshotDto.serializer(),
                        payload,
                    )
                }
                .getOrNull()
                ?.takeIf { it.threadId == threadId }
                ?: continue
        return snapshot.toModel()
    }
    return null
}

/** `resolveVisibleWorktreeSetup` in `client-runtime/src/worktreeSetup.ts`. */
fun resolveVisibleWorktreeSetup(
    live: WorktreeSetupSnapshot?,
    recorded: WorktreeSetupSnapshot?,
    turnStarted: Boolean,
    /** The user sent a message after the one that created the worktree. */
    followUpSent: Boolean,
): WorktreeSetupSnapshot? {
    val snapshot =
        if (live != null && (recorded == null || live.sequence >= recorded.sequence)) {
            live
        } else {
            recorded
        }
    if (snapshot == null) return null
    if (snapshot.phase == "running") return snapshot
    if (followUpSent) return null
    if (snapshot.phase != "done") return snapshot
    if (!turnStarted) return snapshot
    return if (snapshot.stages.any { it.isFailed }) snapshot else null
}
