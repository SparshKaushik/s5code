package club.touchtech.s5code.kotlin.platform.terminal

import android.content.Context
import android.graphics.Color
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import kotlin.math.max

/**
 * Native Ghostty terminal host used directly by Compose.
 *
 * The server remains the PTY owner. This view replays its raw byte-preserving text
 * history into Ghostty, renders the resulting grid on Canvas, and sends Ghostty's
 * device replies and mode-aware hardware-key encodings back to that PTY.
 */
internal class S5TerminalView(context: Context) : FrameLayout(context) {
    private val terminalCanvas = TerminalCanvasView(context)
    private val inputView = EditText(context)
    private var terminalHandle = 0L
    private var replayBuffer = ""
    private var fedBuffer = ""
    private var cols = 0
    private var rows = 0
    private var clearingInput = false
    private var released = false

    var onInput: (String) -> Unit = {}
    var onResize: (Int, Int) -> Unit = { _, _ -> }

    var fontSizeSp: Float = DEFAULT_FONT_SIZE_SP
        set(value) {
            val normalized = value.coerceIn(MIN_FONT_SIZE_SP, MAX_FONT_SIZE_SP)
            if (field == normalized) return
            field = normalized
            terminalCanvas.fontSizeSp = normalized
            inputView.textSize = max(normalized, 13f)
            emitResize()
        }

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        terminalCanvas.fontSizeSp = fontSizeSp
        terminalCanvas.onRequestKeyboard = ::showKeyboard
        terminalCanvas.onScrollRows = { delta ->
            if (terminalHandle != 0L) {
                GhosttyBridge.nativeScroll(terminalHandle, delta)
                renderSnapshot()
            }
        }
        terminalCanvas.onCellMetricsChanged = ::emitResize
        terminalCanvas.selectionDelegate =
            object : TerminalSelectionDelegate {
                override fun selectWordAt(col: Int, row: Int): Boolean {
                    if (terminalHandle == 0L) return false
                    return GhosttyBridge.nativeSelectWordAt(terminalHandle, col, row).also {
                        if (it) renderSnapshot()
                    }
                }

                override fun extendSelection(anchorCol: Int, anchorRow: Int, col: Int, row: Int) {
                    if (terminalHandle == 0L) return
                    GhosttyBridge.nativeExtendSelection(terminalHandle, anchorCol, anchorRow, col, row)
                    renderSnapshot()
                }

                override fun selectAll(): Boolean {
                    if (terminalHandle == 0L) return false
                    return GhosttyBridge.nativeSelectAll(terminalHandle).also {
                        if (it) renderSnapshot()
                    }
                }

                override fun clearSelection() {
                    if (terminalHandle == 0L) return
                    GhosttyBridge.nativeClearSelection(terminalHandle)
                    renderSnapshot()
                }

                override fun selectionText(): String? = selectedText()
            }
        configureInputView()
        addView(
            terminalCanvas,
            LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
        )
        addView(inputView, LayoutParams(1, 1))
    }

    fun setReplayBuffer(value: String) {
        if (replayBuffer == value) return
        replayBuffer = value
        feedPendingBuffer()
    }

    fun setTheme(theme: TerminalTheme) {
        setBackgroundColor(theme.background)
        terminalCanvas.setBackgroundColor(theme.background)
        if (terminalHandle != 0L) {
            GhosttyBridge.nativeSetTheme(
                terminalHandle,
                theme.foreground,
                theme.background,
                theme.cursor,
                theme.palette,
            )
            renderSnapshot()
        }
        currentTheme = theme
    }

    fun selectedText(): String? =
        if (terminalHandle == 0L) null
        else GhosttyBridge.nativeGetSelectionText(terminalHandle)?.toString(Charsets.UTF_8)

    fun copyAllText(): String? {
        if (terminalHandle == 0L || !GhosttyBridge.nativeSelectAll(terminalHandle)) return null
        val result = selectedText()
        GhosttyBridge.nativeClearSelection(terminalHandle)
        terminalCanvas.resetSelectionState()
        renderSnapshot()
        return result
    }

    /** Sends a toolbar key through the same mode-aware path as a hardware keyboard. */
    fun sendKey(keyCode: Int, metaState: Int = 0) {
        val keyEvent = KeyEvent(0L, 0L, KeyEvent.ACTION_DOWN, keyCode, 0, metaState)
        val unshifted = keyEvent.getUnicodeChar(0)
        val request =
            TerminalHardwareKeys.request(
                keyCode = keyCode,
                action = KeyEvent.ACTION_DOWN,
                metaState = metaState,
                repeatCount = 0,
                text = keyEvent.getUnicodeChar(metaState).asTerminalText(),
                unshiftedCodepoint = unshifted,
            ) ?: return
        encodeAndSend(request)
    }

    fun showKeyboard() {
        inputView.requestFocus()
        context.getSystemService(InputMethodManager::class.java)
            ?.showSoftInput(inputView, InputMethodManager.SHOW_IMPLICIT)
    }

    fun hideKeyboard() {
        context.getSystemService(InputMethodManager::class.java)
            ?.hideSoftInputFromWindow(windowToken, 0)
    }

    fun release() {
        if (released) return
        released = true
        inputView.setOnEditorActionListener(null)
        inputView.setOnKeyListener(null)
        terminalCanvas.onScrollRows = null
        terminalCanvas.onRequestKeyboard = null
        terminalCanvas.onCellMetricsChanged = null
        terminalCanvas.selectionDelegate = null
        destroyTerminal()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        if (width != oldWidth || height != oldHeight) emitResize()
    }

    private var currentTheme = TerminalTheme.dark()

    private fun configureInputView() {
        inputView.setSingleLine(true)
        inputView.setTextColor(Color.TRANSPARENT)
        inputView.setHintTextColor(Color.TRANSPARENT)
        inputView.setBackgroundColor(Color.TRANSPARENT)
        inputView.textSize = max(fontSizeSp, 13f)
        inputView.alpha = 0.01f
        inputView.isFocusableInTouchMode = true
        inputView.imeOptions =
            EditorInfo.IME_ACTION_SEND or
                EditorInfo.IME_FLAG_NO_EXTRACT_UI or
                EditorInfo.IME_FLAG_NO_FULLSCREEN or
                EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING
        inputView.inputType =
            InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
                InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        inputView.setPadding(0, 0, 0, 0)
        inputView.setOnEditorActionListener { _, actionId, event ->
            val send = actionId == EditorInfo.IME_ACTION_SEND && event == null
            if (send) onInput("\r")
            send
        }
        inputView.setOnKeyListener { _, keyCode, event ->
            if (!event.isTerminalHardwareEvent()) return@setOnKeyListener false
            val textMeta =
                event.metaState and
                    (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK or KeyEvent.META_META_MASK).inv()
            val unshiftedMeta = textMeta and KeyEvent.META_SHIFT_MASK.inv()
            val request =
                TerminalHardwareKeys.request(
                    keyCode = keyCode,
                    action = event.action,
                    metaState = event.metaState,
                    repeatCount = event.repeatCount,
                    text = event.getUnicodeChar(textMeta).asTerminalText(),
                    unshiftedCodepoint = event.getUnicodeChar(unshiftedMeta),
                ) ?: return@setOnKeyListener false
            encodeAndSend(request)
            true
        }
        inputView.addTextChangedListener(
            object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit

                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                    if (clearingInput || s == null || count <= 0) return
                    val end = (start + count).coerceAtMost(s.length)
                    if (start < end) onInput(s.subSequence(start, end).toString())
                }

                override fun afterTextChanged(editable: Editable?) {
                    if (clearingInput || editable.isNullOrEmpty()) return
                    clearingInput = true
                    editable.clear()
                    clearingInput = false
                }
            }
        )
    }

    private fun encodeAndSend(request: TerminalKeyRequest) {
        if (terminalHandle == 0L) return
        val encoded =
            GhosttyBridge.nativeEncodeKey(
                terminalHandle,
                request.key,
                request.action,
                request.modifiers,
                request.text?.toByteArray(Charsets.UTF_8),
                request.unshiftedCodepoint,
            )
        if (encoded.isNotEmpty()) onInput(encoded.toString(Charsets.UTF_8))
    }

    private fun emitResize() {
        if (width <= 0 || height <= 0 || released) return
        val nextCols =
            (terminalCanvas.usableWidth() / terminalCanvas.cellWidthPx).toInt().coerceIn(2, 400)
        val nextRows =
            (terminalCanvas.usableHeight() / terminalCanvas.cellHeightPx).toInt().coerceIn(2, 200)
        if (nextCols == cols && nextRows == rows && terminalHandle != 0L) return
        cols = nextCols
        rows = nextRows
        if (terminalHandle == 0L) createTerminal()
        else emitResponse(
            GhosttyBridge.nativeResize(
                terminalHandle,
                cols,
                rows,
                terminalCanvas.cellWidthPx.toInt(),
                terminalCanvas.cellHeightPx.toInt(),
            )
        )
        onResize(cols, rows)
        feedPendingBuffer()
        renderSnapshot()
    }

    private fun createTerminal() {
        if (terminalHandle != 0L || cols <= 0 || rows <= 0 || released) return
        terminalHandle =
            GhosttyBridge.nativeCreate(
                cols,
                rows,
                terminalCanvas.cellWidthPx.toInt(),
                terminalCanvas.cellHeightPx.toInt(),
                currentTheme.foreground,
                currentTheme.background,
                currentTheme.cursor,
                currentTheme.palette,
            )
        fedBuffer = ""
    }

    private fun recreateTerminal() {
        destroyTerminal()
        createTerminal()
    }

    private fun destroyTerminal() {
        if (terminalHandle != 0L) GhosttyBridge.nativeDestroy(terminalHandle)
        terminalHandle = 0L
        fedBuffer = ""
        terminalCanvas.resetSelectionState()
    }

    private fun feedPendingBuffer() {
        if (terminalHandle == 0L || replayBuffer == fedBuffer) return
        if (!replayBuffer.startsWith(fedBuffer)) recreateTerminal()
        if (terminalHandle == 0L) return
        val suffix = replayBuffer.substring(fedBuffer.length)
        if (suffix.isNotEmpty()) {
            emitResponse(GhosttyBridge.nativeFeed(terminalHandle, suffix.toByteArray(Charsets.UTF_8)))
            if (terminalCanvas.hasActiveSelection()) {
                GhosttyBridge.nativeClearSelection(terminalHandle)
                terminalCanvas.resetSelectionState()
            }
        }
        fedBuffer = replayBuffer
        renderSnapshot()
    }

    private fun renderSnapshot() {
        if (terminalHandle == 0L) return
        TerminalFrame.decode(GhosttyBridge.nativeSnapshot(terminalHandle))
            ?.let(terminalCanvas::setFrame)
    }

    private fun emitResponse(response: ByteArray) {
        if (response.isNotEmpty()) onInput(response.toString(Charsets.UTF_8))
    }

    private fun KeyEvent.isTerminalHardwareEvent(): Boolean =
        deviceId != KeyCharacterMap.VIRTUAL_KEYBOARD && flags and KeyEvent.FLAG_SOFT_KEYBOARD == 0

    private fun Int.asTerminalText(): String? =
        takeIf { it > 0 && Character.isValidCodePoint(it) }
            ?.let(Character::toChars)
            ?.concatToString()

    companion object {
        const val DEFAULT_FONT_SIZE_SP = 10.5f
        const val MIN_FONT_SIZE_SP = 6f
        const val MAX_FONT_SIZE_SP = 14f
    }
}
