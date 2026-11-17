package club.touchtech.s5code.kotlin.feature.thread

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import club.touchtech.s5code.kotlin.data.CommandOutputView
import club.touchtech.s5code.kotlin.data.TERMINAL_PALETTE_DARK
import club.touchtech.s5code.kotlin.data.TERMINAL_PALETTE_LIGHT
import club.touchtech.s5code.kotlin.data.TerminalSpan
import club.touchtech.s5code.kotlin.data.terminalOutputSpans
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow

// Within this many dp of the end still counts as following the output.
private val FOLLOW_SLACK = 8.dp

/**
 * Output of one expanded command row: streams while the command runs, then
 * shows its final output. Ported from `ThreadCommandOutput` in
 * `apps/mobile/src/features/threads/thread-command-output.tsx`. It collects only
 * while composed, so collapsed rows and other threads receive nothing; mount it
 * only for servers advertising `commandOutputStreaming`.
 */
@Composable
internal fun CommandOutputBlock(output: Flow<CommandOutputView>) {
    var view by remember(output) { mutableStateOf<CommandOutputView?>(null) }
    var error by remember(output) { mutableStateOf<String?>(null) }
    LaunchedEffect(output) {
        try {
            output.collect { view = it }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            error = failure.message ?: "unknown error"
        }
    }
    val text = view?.output?.text.orEmpty()
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val style = S5Theme.code.codeSmall
    if (text.isEmpty()) {
        val message =
            when {
                error != null -> "Couldn't load output: $error"
                view == null -> "Loading output…"
                view?.running == true -> null
                else -> "No output."
            } ?: return
        Text(message, style = style, color = muted, modifier = Modifier.padding(top = S5Theme.spacing.small))
        return
    }
    val dark = MaterialTheme.colorScheme.surface.luminance() < 0.5f
    val annotated =
        remember(text, dark, muted) {
            terminalAnnotatedString(
                terminalOutputSpans(text, if (dark) TERMINAL_PALETTE_DARK else TERMINAL_PALETTE_LIGHT),
                muted,
            )
        }
    val scroll = rememberScrollState()
    // Follow new output until the reader scrolls up; scrolling back down resumes.
    var follow by remember(output) { mutableStateOf(true) }
    val slack = with(LocalDensity.current) { FOLLOW_SLACK.roundToPx() }
    LaunchedEffect(scroll) {
        snapshotFlow { scroll.isScrollInProgress to scroll.value }.collect { (dragging, value) ->
            if (dragging) follow = scroll.maxValue - value <= slack
        }
    }
    LaunchedEffect(scroll) {
        snapshotFlow { scroll.maxValue }.collect { if (follow) scroll.scrollTo(it) }
    }
    Column(Modifier.padding(top = S5Theme.spacing.small)) {
        if (view?.output?.truncated == true) {
            Text(
                "Earlier output not shown",
                style = MaterialTheme.typography.labelSmall,
                color = muted,
                modifier = Modifier.padding(bottom = 2.dp),
            )
        }
        SelectionContainer {
            Text(
                annotated,
                style = style,
                color = muted,
                modifier = Modifier.heightIn(max = 240.dp).verticalScroll(scroll).padding(end = 8.dp),
            )
        }
    }
}

private fun terminalAnnotatedString(spans: List<TerminalSpan>, base: Color): AnnotatedString =
    buildAnnotatedString {
        for (span in spans) {
            val style = span.style
            if (style == null) {
                append(span.text)
                continue
            }
            val color = style.color?.let(::Color) ?: base
            val decorations =
                listOfNotNull(
                    TextDecoration.Underline.takeIf { style.underline },
                    TextDecoration.LineThrough.takeIf { style.strike },
                )
            withStyle(
                SpanStyle(
                    color = if (style.dim) color.copy(alpha = 0.65f) else color,
                    background = style.backgroundColor?.let(::Color) ?: Color.Unspecified,
                    fontWeight = if (style.bold) FontWeight.Bold else null,
                    fontStyle = if (style.italic) FontStyle.Italic else null,
                    textDecoration = decorations.takeIf { it.isNotEmpty() }?.let(TextDecoration::combine),
                )
            ) {
                append(span.text)
            }
        }
    }
