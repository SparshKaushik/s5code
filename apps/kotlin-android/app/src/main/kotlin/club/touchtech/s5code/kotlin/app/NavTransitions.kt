package club.touchtech.s5code.kotlin.app

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.IntOffset
import androidx.navigation.NavBackStackEntry
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * Destination transitions: an opaque stack push, the way a native Android stack
 * moves. A screen is a card that slides in over the one below it; nothing
 * dissolves, because a fade is what you use when content changes in place, not
 * when you move between places.
 *
 * The incoming screen never animates alpha. That is the whole point: a fading
 * page shows the outgoing page through it for the duration, which is what read
 * as slow and mushy even at a short duration. The covered screen only drifts a
 * fraction of the width and dims a little, so the pair reads as one card over
 * another rather than two screens sliding in lockstep.
 */

/** How far the covered screen travels, as a fraction of the container width. */
private const val OUTGOING_SLIDE_FRACTION = 5

/**
 * Alpha the covered screen dims to. Only the screen underneath dims, and only
 * slightly: it is depth, not a crossfade.
 */
private const val COVERED_ALPHA = 0.7f

/** Push duration, matching the Android platform activity/fragment push. */
private const val PUSH_MILLIS = 220

/** Pop is a touch quicker: going back should feel like a dismissal. */
private const val POP_MILLIS = 190

/** Reduced motion still marks the change, but without travel. */
private const val REDUCED_MILLIS = 100

@Composable
fun s5NavTransitions(): S5NavTransitions {
    val reduced = S5Theme.reducedMotion

    // Deliberately tweens rather than the theme's spatial springs. The expressive
    // springs are underdamped, and an overshoot on a full-width translation
    // carries the incoming screen past its resting offset, flashing a sliver of
    // the container behind it at the screen edge. Springs are right for a control
    // that settles in place; a page that starts off screen wants a bounded curve.
    val push = tween<IntOffset>(PUSH_MILLIS, easing = FastOutSlowInEasing)
    val pop = tween<IntOffset>(POP_MILLIS, easing = FastOutSlowInEasing)
    // Only ever applied to the screen underneath, and only as a dim.
    val dim = tween<Float>(PUSH_MILLIS, easing = FastOutSlowInEasing)
    val quickFade = tween<Float>(REDUCED_MILLIS)

    if (reduced) {
        return S5NavTransitions(
            enter = { fadeIn(quickFade) },
            exit = { fadeOut(quickFade) },
            popEnter = { fadeIn(quickFade) },
            popExit = { fadeOut(quickFade) },
        )
    }

    return S5NavTransitions(
        // Arriving and leaving screens translate only. No alpha on the moving
        // card, so it is never see-through over the screen it covers.
        enter = {
            slideIntoContainer(
                AnimatedContentTransitionScope.SlideDirection.Start,
                animationSpec = push,
            )
        },
        exit = {
            slideOutOfContainer(
                AnimatedContentTransitionScope.SlideDirection.Start,
                animationSpec = push,
                targetOffset = { it / OUTGOING_SLIDE_FRACTION },
            ) + fadeOut(dim, targetAlpha = COVERED_ALPHA)
        },
        popEnter = {
            slideIntoContainer(
                AnimatedContentTransitionScope.SlideDirection.End,
                animationSpec = pop,
                initialOffset = { it / OUTGOING_SLIDE_FRACTION },
            ) + fadeIn(dim, initialAlpha = COVERED_ALPHA)
        },
        popExit = {
            slideOutOfContainer(
                AnimatedContentTransitionScope.SlideDirection.End,
                animationSpec = pop,
            )
        },
    )
}

/**
 * The four transition slots a [androidx.navigation.compose.NavHost] takes,
 * bundled so the graph wires them in one place and cannot use three of four.
 */
data class S5NavTransitions(
    val enter: AnimatedContentTransitionScope<NavBackStackEntry>.() -> EnterTransition,
    val exit: AnimatedContentTransitionScope<NavBackStackEntry>.() -> ExitTransition,
    val popEnter: AnimatedContentTransitionScope<NavBackStackEntry>.() -> EnterTransition,
    val popExit: AnimatedContentTransitionScope<NavBackStackEntry>.() -> ExitTransition,
)
