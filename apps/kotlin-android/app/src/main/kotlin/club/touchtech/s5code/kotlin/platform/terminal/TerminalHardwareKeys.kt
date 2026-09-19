package club.touchtech.s5code.kotlin.platform.terminal

import android.view.KeyEvent

/** A platform key event normalized for Ghostty's terminal-mode-aware encoder. */
internal data class TerminalKeyRequest(
    val key: Int,
    val action: Int,
    val modifiers: Int,
    val text: String?,
    val unshiftedCodepoint: Int,
)

/**
 * Hardware keyboards must follow the active terminal modes. In particular, arrows
 * switch to SS3 in application-cursor mode and Kitty keyboard protocol can request
 * release events. The Android event is reduced to stable semantic ids here; JNI
 * maps those ids to Ghostty's public key enum and lets Ghostty encode the bytes.
 */
internal object TerminalHardwareKeys {
    const val ACTION_RELEASE = 0
    const val ACTION_PRESS = 1
    const val ACTION_REPEAT = 2

    private const val MOD_SHIFT = 1 shl 0
    private const val MOD_CTRL = 1 shl 1
    private const val MOD_ALT = 1 shl 2
    private const val MOD_SUPER = 1 shl 3
    private const val MOD_CAPS_LOCK = 1 shl 4
    private const val MOD_NUM_LOCK = 1 shl 5

    private const val KEY_BACKSPACE = 1
    private const val KEY_ENTER = 2
    private const val KEY_TAB = 3
    private const val KEY_DELETE = 4
    private const val KEY_END = 5
    private const val KEY_HOME = 6
    private const val KEY_INSERT = 7
    private const val KEY_PAGE_DOWN = 8
    private const val KEY_PAGE_UP = 9
    private const val KEY_ARROW_DOWN = 10
    private const val KEY_ARROW_LEFT = 11
    private const val KEY_ARROW_RIGHT = 12
    private const val KEY_ARROW_UP = 13
    private const val KEY_ESCAPE = 14
    private const val KEY_SPACE = 15
    private const val KEY_LETTER_BASE = 100
    private const val KEY_DIGIT_BASE = 200
    private const val KEY_FUNCTION_BASE = 300
    private const val KEY_PUNCTUATION_BASE = 400

    /** Returns null only for keys that should stay in Android (volume, media, etc.). */
    fun request(
        keyCode: Int,
        action: Int,
        metaState: Int,
        repeatCount: Int,
        text: String?,
        unshiftedCodepoint: Int,
    ): TerminalKeyRequest? {
        val key = semanticKey(keyCode) ?: return null
        val nativeAction =
            when (action) {
                KeyEvent.ACTION_UP -> ACTION_RELEASE
                KeyEvent.ACTION_DOWN -> if (repeatCount > 0) ACTION_REPEAT else ACTION_PRESS
                else -> return null
            }
        return TerminalKeyRequest(
            key = key,
            action = nativeAction,
            modifiers = modifiers(metaState),
            text = text?.takeIf { candidate -> candidate.all { it >= ' ' && it != '\u007f' } },
            unshiftedCodepoint = unshiftedCodepoint.coerceAtLeast(0),
        )
    }

    private fun semanticKey(keyCode: Int): Int? =
        when (keyCode) {
            KeyEvent.KEYCODE_DEL -> KEY_BACKSPACE
            KeyEvent.KEYCODE_ENTER,
            KeyEvent.KEYCODE_NUMPAD_ENTER -> KEY_ENTER
            KeyEvent.KEYCODE_TAB -> KEY_TAB
            KeyEvent.KEYCODE_FORWARD_DEL -> KEY_DELETE
            KeyEvent.KEYCODE_MOVE_END -> KEY_END
            KeyEvent.KEYCODE_MOVE_HOME -> KEY_HOME
            KeyEvent.KEYCODE_INSERT -> KEY_INSERT
            KeyEvent.KEYCODE_PAGE_DOWN -> KEY_PAGE_DOWN
            KeyEvent.KEYCODE_PAGE_UP -> KEY_PAGE_UP
            KeyEvent.KEYCODE_DPAD_DOWN -> KEY_ARROW_DOWN
            KeyEvent.KEYCODE_DPAD_LEFT -> KEY_ARROW_LEFT
            KeyEvent.KEYCODE_DPAD_RIGHT -> KEY_ARROW_RIGHT
            KeyEvent.KEYCODE_DPAD_UP -> KEY_ARROW_UP
            KeyEvent.KEYCODE_ESCAPE -> KEY_ESCAPE
            KeyEvent.KEYCODE_SPACE -> KEY_SPACE
            in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z ->
                KEY_LETTER_BASE + keyCode - KeyEvent.KEYCODE_A
            in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 ->
                KEY_DIGIT_BASE + keyCode - KeyEvent.KEYCODE_0
            in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 ->
                KEY_FUNCTION_BASE + keyCode - KeyEvent.KEYCODE_F1
            KeyEvent.KEYCODE_GRAVE -> KEY_PUNCTUATION_BASE
            KeyEvent.KEYCODE_BACKSLASH -> KEY_PUNCTUATION_BASE + 1
            KeyEvent.KEYCODE_LEFT_BRACKET -> KEY_PUNCTUATION_BASE + 2
            KeyEvent.KEYCODE_RIGHT_BRACKET -> KEY_PUNCTUATION_BASE + 3
            KeyEvent.KEYCODE_COMMA -> KEY_PUNCTUATION_BASE + 4
            KeyEvent.KEYCODE_EQUALS -> KEY_PUNCTUATION_BASE + 5
            KeyEvent.KEYCODE_MINUS -> KEY_PUNCTUATION_BASE + 6
            KeyEvent.KEYCODE_PERIOD -> KEY_PUNCTUATION_BASE + 7
            KeyEvent.KEYCODE_APOSTROPHE -> KEY_PUNCTUATION_BASE + 8
            KeyEvent.KEYCODE_SEMICOLON -> KEY_PUNCTUATION_BASE + 9
            KeyEvent.KEYCODE_SLASH -> KEY_PUNCTUATION_BASE + 10
            else -> null
        }

    private fun modifiers(metaState: Int): Int {
        var result = 0
        if (metaState and KeyEvent.META_SHIFT_ON != 0) result = result or MOD_SHIFT
        if (metaState and KeyEvent.META_CTRL_ON != 0) result = result or MOD_CTRL
        if (metaState and KeyEvent.META_ALT_ON != 0) result = result or MOD_ALT
        if (metaState and KeyEvent.META_META_ON != 0) result = result or MOD_SUPER
        if (metaState and KeyEvent.META_CAPS_LOCK_ON != 0) result = result or MOD_CAPS_LOCK
        if (metaState and KeyEvent.META_NUM_LOCK_ON != 0) result = result or MOD_NUM_LOCK
        return result
    }
}
