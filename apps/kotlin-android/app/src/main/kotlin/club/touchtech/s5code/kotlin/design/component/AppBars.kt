package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.RowScope
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarScrollBehavior
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow

/**
 * Title prominence for a destination's top bar.
 *
 * The scale stops at the medium flexible bar. The large flexible bar's expanded
 * title is display-sized, which reads as a magazine headline above a working
 * list and costs a third of a phone screen before any content shows. A tool's
 * header should say where you are and get out of the way.
 *
 * - [Hero]: medium flexible bar that collapses on scroll, for a screen whose
 *   title is the page identity (home, settings root).
 * - [Section]: single-row bar, the default for pushed destinations.
 * - [Compact]: single-row bar for dense tool surfaces (file viewer, terminal).
 * - [Centered]: single-row centered bar, for sheet-like destinations.
 *
 * [Section] and [Compact] render the same bar today. They stay distinct because
 * they answer different questions: "this is a normal pushed screen" versus "this
 * screen is a dense tool". Collapsing them would lose that intent.
 */
enum class S5TopBarProminence {
    Hero,
    Section,
    Compact,
    Centered,
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5TopAppBar(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    prominence: S5TopBarProminence = S5TopBarProminence.Section,
    onBack: (() -> Unit)? = null,
    backLabel: String = "Back",
    scrollBehavior: TopAppBarScrollBehavior? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    // The hero bar's collapsed row is the same fixed height as a single-row bar,
    // so a two-line title there either clips or pushes the bar taller than the
    // scale allows. Two lines belong to the expanded state only, and the title
    // follows the collapse rather than picking one answer for both.
    //
    // Derived to a boolean, not read as a float: the raw fraction changes every
    // frame of a scroll, and reading it here would recompose the title on each
    // one to arrive at the same two values.
    val allowsTwoLineTitle by
        remember(scrollBehavior, prominence) {
            derivedStateOf {
                prominence == S5TopBarProminence.Hero &&
                    scrollBehavior != null &&
                    scrollBehavior.state.collapsedFraction < TITLE_TWO_LINE_FRACTION
            }
        }
    val titleContent: @Composable () -> Unit = {
        Text(
            title,
            maxLines = if (allowsTwoLineTitle) 2 else 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
    val subtitleContent: (@Composable () -> Unit)? =
        subtitle?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } }
    val navigationIcon: @Composable () -> Unit = {
        if (onBack != null) {
            S5IconButton(
                icon = Icons.AutoMirrored.Rounded.ArrowBack,
                label = backLabel,
                onClick = onBack,
            )
        }
    }

    when (prominence) {
        S5TopBarProminence.Hero ->
            MediumFlexibleTopAppBar(
                title = titleContent,
                subtitle = subtitleContent,
                modifier = modifier,
                navigationIcon = navigationIcon,
                actions = actions,
                scrollBehavior = scrollBehavior,
            )
        S5TopBarProminence.Section,
        S5TopBarProminence.Compact ->
            if (subtitleContent != null) {
                TopAppBar(
                    title = titleContent,
                    subtitle = subtitleContent,
                    modifier = modifier,
                    navigationIcon = navigationIcon,
                    actions = actions,
                    scrollBehavior = scrollBehavior,
                )
            } else {
                TopAppBar(
                    title = titleContent,
                    modifier = modifier,
                    navigationIcon = navigationIcon,
                    actions = actions,
                    scrollBehavior = scrollBehavior,
                )
            }
        // Centered bars have no subtitle slot; sheet-like destinations keep the
        // secondary line in their content instead.
        S5TopBarProminence.Centered ->
            CenterAlignedTopAppBar(
                title = titleContent,
                modifier = modifier,
                navigationIcon = navigationIcon,
                actions = actions,
                scrollBehavior = scrollBehavior,
            )
    }
}

/**
 * How far the hero bar must expand before its title may take a second line. Set
 * below halfway so the second line arrives with the expanding bar rather than
 * appearing after it has already made room.
 */
private const val TITLE_TWO_LINE_FRACTION = 0.4f
