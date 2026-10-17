package club.touchtech.s5code.kotlin.design.component

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FloatingToolbarDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberTopAppBarState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * The standard destination frame: top app bar, edge-to-edge insets, an optional
 * hero FAB, and an optional floating toolbar. Screens supply content; they never
 * assemble their own Scaffold so insets and app-bar behavior stay consistent
 * across every destination.
 *
 * Only [S5TopBarProminence.Hero] collapses on scroll. A single-row bar has
 * nothing to collapse into, and `exitUntilCollapsed` on one only makes the title
 * slide away on the first flick.
 *
 * [belowTopBar] is a pinned strip between the bar and the content, for state that
 * must stay reachable once content scrolls past it (the thread's active plan
 * step). It sits in the Scaffold's top slot, so the content padding already
 * accounts for its height. It receives the bar's collapsed fraction (0 expanded,
 * 1 fully collapsed) so a strip can appear exactly as the tall title gives up
 * its space, rather than guessing from a scroll offset.
 *
 * [topBarCollapsed] drives the bar's height directly, for content whose scroll
 * direction does not map onto "further down the list". A reversed transcript is
 * the case that needs it: gesture-driven collapse alone leaves the bar expanded
 * on arrival at the live tail, which is exactly where the compact bar belongs.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5Screen(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    prominence: S5TopBarProminence = S5TopBarProminence.Section,
    onBack: (() -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
    floatingActionButton: @Composable () -> Unit = {},
    bottomBar: @Composable () -> Unit = {},
    /** Global loading strip above this destination's content. */
    loading: Boolean = false,
    loadingProgress: Float? = null,
    topBarCollapsed: Boolean? = null,
    belowTopBar: @Composable (collapsedFraction: Float) -> Unit = {},
    content: @Composable (PaddingValues) -> Unit,
) {
    val topAppBarState = rememberTopAppBarState()
    val scrollBehavior =
        if (prominence == S5TopBarProminence.Hero) {
            TopAppBarDefaults.exitUntilCollapsedScrollBehavior(topAppBarState)
        } else {
            TopAppBarDefaults.pinnedScrollBehavior(topAppBarState)
        }
    // Derived so only the strip recomposes as the bar collapses, not the screen.
    val collapsedFraction by remember { derivedStateOf { topAppBarState.collapsedFraction } }
    // The limit is negative and only known after the bar measures, so it is a
    // key: the first pass has nothing to animate toward.
    val heightOffsetLimit = topAppBarState.heightOffsetLimit
    val reducedMotion = S5Theme.reducedMotion
    LaunchedEffect(topBarCollapsed, heightOffsetLimit, reducedMotion) {
        if (topBarCollapsed == null || heightOffsetLimit == 0f) return@LaunchedEffect
        val target = if (topBarCollapsed) heightOffsetLimit else 0f
        if (reducedMotion) {
            topAppBarState.heightOffset = target
            return@LaunchedEffect
        }
        animate(
            initialValue = topAppBarState.heightOffset,
            targetValue = target,
            animationSpec = tween(TOP_BAR_MILLIS, easing = FastOutSlowInEasing),
        ) { value, _ ->
            topAppBarState.heightOffset = value
        }
    }
    Scaffold(
        modifier = modifier,
        topBar = {
            Column {
                S5TopAppBar(
                    title = title,
                    subtitle = subtitle,
                    prominence = prominence,
                    onBack = onBack,
                    scrollBehavior = scrollBehavior,
                    actions = actions,
                )
                if (loading) S5LoadingStrip(progress = loadingProgress)
                belowTopBar(collapsedFraction)
            }
        },
        bottomBar = bottomBar,
        floatingActionButton = floatingActionButton,
        // The nested-scroll connection wraps the content only, never the whole
        // Scaffold. A bottom bar can be scrollable in its own right — the thread
        // composer's multiline field is — and a Scaffold-wide connection lets that
        // field's internal scroll drive the app bar, so dragging inside the message
        // box expanded the header.
        content = { padding ->
            Box(Modifier.fillMaxSize().nestedScroll(scrollBehavior.nestedScrollConnection)) {
                content(padding)
            }
        },
    )
}

/** How long a driven bar takes to collapse or expand. Matches the push. */
private const val TOP_BAR_MILLIS = 220

/**
 * Overlay slot for a screen-level floating toolbar. Placed by the caller so the
 * toolbar keeps the FAB spacing the expressive spec expects.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5ToolbarOverlay(modifier: Modifier = Modifier, content: @Composable BoxScope.() -> Unit) {
    Box(
        modifier
            .fillMaxSize()
            .padding(bottom = FloatingToolbarDefaults.ScreenOffset),
        content = content,
    )
}
