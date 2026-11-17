package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow

/**
 * A [TextFieldState] for a field whose text is owned elsewhere (a store draft).
 *
 * The state-based text field is not a convenience here: keyboard content commits
 * (Gboard's clipboard, sticker, and GIF insertion) only reach a field backed by
 * `TextFieldState`. The legacy `value`/`onValueChange` input path refuses them,
 * so a composer that wants the OS paste gesture has to hold state.
 *
 * Edits publish through [onValueChange]; an external change to [value] is
 * applied with the caret at the end. The last published text guards the loop, so
 * the echo of the user's own keystroke never re-applies and moves the caret
 * mid-typing.
 *
 * [key] resets the state, e.g. per thread.
 */
@Composable
fun rememberDraftTextFieldState(
    key: Any?,
    value: String,
    onValueChange: (String) -> Unit,
): TextFieldState {
    val state = remember(key) { TextFieldState(value) }
    val published = remember(key) { arrayOf(value) }

    LaunchedEffect(state) {
        snapshotFlow { state.text.toString() }
            .collect { text ->
                if (text != published[0]) {
                    published[0] = text
                    onValueChange(text)
                }
            }
    }
    LaunchedEffect(state, value) {
        if (value != published[0]) {
            published[0] = value
            state.setTextAndPlaceCursorAtEnd(value)
        }
    }

    return state
}
