package club.touchtech.s5code.kotlin.app

import kotlin.math.roundToInt

/** Width constraints shared by the expanded workspace and its resize divider. */
internal object WorkspacePaneSizing {
    const val ListWidthDp = 400
    const val InspectorMinWidthDp = 260
    const val InspectorMaxWidthDp = 480
    const val InspectorDefaultMaxWidthDp = 320
    const val MinimumChatWidthDp = 560

    fun defaultInspectorWidth(availableWidthDp: Float): Float =
        constrainInspectorWidth(
            preferredWidthDp =
                (availableWidthDp * 0.28f)
                    .roundToInt()
                    .coerceIn(InspectorMinWidthDp, InspectorDefaultMaxWidthDp)
                    .toFloat(),
            availableWidthDp = availableWidthDp,
        )

    /** Never lets the inspector squeeze chat below its usable minimum. */
    fun constrainInspectorWidth(preferredWidthDp: Float, availableWidthDp: Float): Float {
        val safeAvailable = availableWidthDp.takeIf(Float::isFinite)?.coerceAtLeast(0f) ?: 0f
        val safePreferred =
            preferredWidthDp.takeIf(Float::isFinite) ?: InspectorMinWidthDp.toFloat()
        val maxWidth =
            (safeAvailable - MinimumChatWidthDp)
                .coerceIn(InspectorMinWidthDp.toFloat(), InspectorMaxWidthDp.toFloat())
        return safePreferred.coerceIn(InspectorMinWidthDp.toFloat(), maxWidth)
    }
}

/** Aux destinations that remain beside an active thread on expanded windows. */
internal fun isWorkspaceInspectorRoute(route: String?): Boolean =
    route != null &&
        (route == Routes.ThreadFiles ||
            route == Routes.ThreadFile ||
            route == Routes.ThreadFileMarkdown ||
            route == Routes.ThreadFileImage ||
            route == Routes.ThreadFileWeb ||
            route == Routes.ThreadReview ||
            route == Routes.ThreadReviewComment ||
            route == Routes.ThreadTerminal ||
            route == Routes.Git ||
            route == Routes.GitCommit ||
            route == Routes.GitBranches ||
            route == Routes.GitConfirm ||
            route == Routes.SourceControl ||
            route == Routes.PullRequests)

internal fun isWorkspaceInspectorSuffix(suffix: String): Boolean =
    suffix.substringBefore('?') in
        setOf(
            "files",
            "files/source",
            "files/markdown",
            "files/image",
            "files/web",
            "review",
            "review-comment",
            "terminal",
            "git",
            "git/commit",
            "git/branches",
            "git-confirm",
            "source-control",
            "pull-requests",
        )
