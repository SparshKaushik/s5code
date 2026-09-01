package club.touchtech.s5code.kotlin.app

import android.view.KeyEvent

/** App-level actions emitted by a physical keyboard. */
enum class S5HardwareShortcut {
    NewTask,
    FocusSearch,
    Escape,
}

/**
 * A sequenced shortcut event. The sequence makes pressing the same key twice a
 * distinct Compose state change.
 */
data class S5HardwareShortcutEvent(
    val id: Long,
    val shortcut: S5HardwareShortcut,
)

/**
 * Resolves only app-global keys. Composer submit stays on the focused composer,
 * while terminal keys stay in Ghostty's native input view.
 */
internal fun resolveHardwareShortcut(
    keyCode: Int,
    action: Int,
    repeatCount: Int,
    ctrlPressed: Boolean,
    metaPressed: Boolean,
    altPressed: Boolean,
): S5HardwareShortcut? {
    if (action != KeyEvent.ACTION_DOWN || repeatCount != 0) return null

    val commandPressed = ctrlPressed || metaPressed
    return when {
        keyCode == KeyEvent.KEYCODE_ESCAPE -> S5HardwareShortcut.Escape
        !altPressed && commandPressed && keyCode == KeyEvent.KEYCODE_N ->
            S5HardwareShortcut.NewTask
        !altPressed && commandPressed && keyCode == KeyEvent.KEYCODE_K ->
            S5HardwareShortcut.FocusSearch
        else -> null
    }
}

/** Cmd/Ctrl+Enter submits only from the focused composer. */
internal fun isComposerSubmitShortcut(
    keyCode: Int,
    action: Int,
    repeatCount: Int,
    ctrlPressed: Boolean,
    metaPressed: Boolean,
    altPressed: Boolean,
): Boolean =
    action == KeyEvent.ACTION_DOWN &&
        repeatCount == 0 &&
        !altPressed &&
        (ctrlPressed || metaPressed) &&
        (keyCode == KeyEvent.KEYCODE_ENTER || keyCode == KeyEvent.KEYCODE_NUMPAD_ENTER)
