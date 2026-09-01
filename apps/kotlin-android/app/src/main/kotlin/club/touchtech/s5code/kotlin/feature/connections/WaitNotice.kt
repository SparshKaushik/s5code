package club.touchtech.s5code.kotlin.feature.connections

import club.touchtech.s5code.kotlin.model.ConnectionState

/**
 * What a screen is waiting on, when it has nothing better to show.
 *
 * Ported from the RN client's two places that answer this question the same way:
 * `apps/mobile/src/features/connection/EnvironmentConnectionNotice.tsx` for the
 * full-screen case and `composerConnectionStatus` in
 * `apps/mobile/src/features/threads/ThreadComposer.tsx` for the pill over live
 * content. One derivation for both, because a screen that says "Reconnecting" in
 * the middle and "Offline" at the bottom is a screen the user cannot trust.
 */
enum class WaitPhase {
    /** First connection attempt to an environment that has never been reached. */
    Connecting,

    /** A known-good environment dropped and the transport is retrying. */
    Reconnecting,

    /** No usable transport. Persisted or in-memory snapshots may still be shown. */
    Offline,

    /** The environment answered, but the pairing or token is no longer valid. */
    SignInNeeded,

    /** Connected, nothing loaded yet, waiting on the first snapshot. */
    Loading,
}

/**
 * A phase with the words to render for it. [spinning] is what decides between an
 * expressive indicator and a static icon: a phase that is not making progress must
 * not animate, since a spinner on a dead connection is the lying spinner.
 */
data class WaitNotice(val phase: WaitPhase, val title: String, val detail: String) {
    val spinning: Boolean
        get() =
            when (phase) {
                WaitPhase.Connecting,
                WaitPhase.Reconnecting,
                WaitPhase.Loading -> true
                WaitPhase.Offline,
                WaitPhase.SignInNeeded -> false
            }
}

/**
 * The notice for a set of environment connections, or null when there is nothing to
 * say — connected with content already on screen.
 *
 * [states] is every environment the screen draws from, so the home list can pass all
 * of them and a thread can pass its one. The best state wins: with one environment
 * connected and another offline, the screen is working, and a warning about the
 * other one belongs on Connections rather than over the list.
 *
 * [hasContent] separates the two shapes. Without content the notice is the whole
 * screen; with it, the notice is a pill and "Loading" becomes "Syncing", matching
 * the RN composer pill's one-stable-label-per-open rule.
 *
 * [resourceName] names what is being waited on ("threads", "transcript"), so the
 * detail line reads as a sentence rather than as generic chrome.
 */
fun waitNotice(
    states: List<ConnectionState>,
    environmentLabel: String?,
    resourceName: String,
    hasContent: Boolean,
    /**
     * True when the device has environments saved but none of their sessions exist
     * yet — the first frames after a cold start, before the store's list has been
     * reconciled into sessions.
     *
     * Without this the home screen answered "no threads yet" for those frames,
     * which is the same lie as a spinner on a dead connection told backwards: the
     * app claiming an empty account while it is still opening its sockets.
     */
    awaitingEnvironments: Boolean = false,
): WaitNotice? {
    val label = environmentLabel ?: "the environment"
    if (states.isEmpty()) {
        if (!awaitingEnvironments) return null
        return WaitNotice(
            WaitPhase.Connecting,
            "Connecting to $label",
            "The $resourceName will load as soon as the environment is ready.",
        )
    }
    val best = states.minByOrNull { it.waitRank } ?: return null
    return when (best) {
        ConnectionState.Connected ->
            if (hasContent) null
            else
                WaitNotice(
                    WaitPhase.Loading,
                    "Loading $resourceName",
                    "Reading the latest state from $label.",
                )
        ConnectionState.Connecting ->
            WaitNotice(
                WaitPhase.Connecting,
                "Connecting to $label",
                "The $resourceName will load as soon as the environment is ready.",
            )
        ConnectionState.Recovering ->
            WaitNotice(
                WaitPhase.Reconnecting,
                "Reconnecting to $label",
                // Cached snapshots are restored before the socket catches up;
                // reconnecting never clears them.
                "The connection dropped. Retrying automatically, and cached chats stay on screen.",
            )
        ConnectionState.Offline ->
            WaitNotice(
                WaitPhase.Offline,
                "You are offline",
                "Cached chats stay on screen. The $resourceName will refresh when your connection returns.",
            )
        ConnectionState.AuthRequired ->
            WaitNotice(
                WaitPhase.SignInNeeded,
                "$label needs sign-in again",
                "Pair this device with $label to load the $resourceName.",
            )
    }
}

/**
 * Short label for the pill form, which has one line and no room for a sentence.
 *
 * The titles are already short enough for every phase except the first load, which
 * only reaches the pill when content is on screen — and that is the case the RN
 * client labels "Syncing" rather than "Loading".
 */
fun waitPillLabel(notice: WaitNotice): String =
    if (notice.phase == WaitPhase.Loading) "Syncing…" else notice.title

/**
 * Ordering for "which of these connections is the screen actually using". Lower is
 * better. Deliberately not the enum's declaration order, which is the lifecycle
 * order and puts `Connecting` ahead of `Connected`.
 */
private val ConnectionState.waitRank: Int
    get() =
        when (this) {
            ConnectionState.Connected -> 0
            ConnectionState.Connecting -> 1
            ConnectionState.Recovering -> 2
            ConnectionState.AuthRequired -> 3
            ConnectionState.Offline -> 4
        }
