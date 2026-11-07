package club.touchtech.s5code.kotlin.design.component

import android.content.ClipData
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.toClipEntry
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import kotlinx.coroutines.launch

/**
 * Copy-text callback for screens. The Compose clipboard write is suspending, so
 * this hides the scope hop and keeps every copy affordance on one code path.
 */
@Composable
fun rememberClipboardWriter(): (String) -> Unit {
    val clipboard = LocalClipboard.current
    val haptics = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    return remember(clipboard, haptics, scope) {
        { text: String ->
            scope.launch {
                clipboard.setClipEntry(ClipData.newPlainText(text, text).toClipEntry())
                haptics.performHapticFeedback(HapticFeedbackType.Confirm)
            }
        }
    }
}
