package club.touchtech.s5code.kotlin

import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import androidx.compose.runtime.mutableStateOf
import club.touchtech.s5code.kotlin.app.DeepLink
import club.touchtech.s5code.kotlin.app.S5App
import club.touchtech.s5code.kotlin.app.S5HardwareShortcut
import club.touchtech.s5code.kotlin.app.S5HardwareShortcutEvent
import club.touchtech.s5code.kotlin.app.S5WindowWidth
import club.touchtech.s5code.kotlin.app.resolveHardwareShortcut
import club.touchtech.s5code.kotlin.platform.resolveIntentLink

class MainActivity : ComponentActivity() {
    /**
     * The link the current intent asked for. State rather than a field so a warm
     * start (`onNewIntent`) reaches the composition: the activity is
     * `singleTask`, so a second shortcut tap or notification does not recreate it.
     */
    private val pendingLink = mutableStateOf<DeepLink?>(null)
    private val hardwareShortcut = mutableStateOf<S5HardwareShortcutEvent?>(null)
    private var hardwareShortcutSequence = 0L
    private var consumedShortcutKeyCode: Int? = null

    @OptIn(ExperimentalMaterial3WindowSizeClassApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // The activity launches on Theme.S5Code.Launch, which carries the splash.
        // Swapping to the plain theme here drops the splash window background,
        // so a later configuration change or a return from recents does not
        // repaint the wordmark behind the live UI.
        setTheme(R.style.Theme_S5Code)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // Fade the platform splash out instead of cutting: without this the
            // splash is removed the instant the first frame is ready, which on a
            // fast device reads as a flicker.
            fadeOutSplashScreen()
        }
        // Only on a cold start: a recreation from a configuration change carries
        // the original intent, and re-navigating on every rotation would fight
        // the user.
        if (savedInstanceState == null) pendingLink.value = resolveIntentLink(intent)
        setContent {
            val sizeClass = calculateWindowSizeClass(this)
            S5App(
                widthSizeClass =
                    when (sizeClass.widthSizeClass) {
                        WindowWidthSizeClass.Expanded -> S5WindowWidth.Expanded
                        WindowWidthSizeClass.Medium -> S5WindowWidth.Medium
                        else -> S5WindowWidth.Compact
                    },
                pendingLink = pendingLink.value,
                onLinkConsumed = { pendingLink.value = null },
                hardwareShortcut = hardwareShortcut.value,
            )
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        resolveIntentLink(intent)?.let { pendingLink.value = it }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val shortcut =
            resolveHardwareShortcut(
                keyCode = keyCode,
                action = event.action,
                repeatCount = event.repeatCount,
                ctrlPressed = event.isCtrlPressed,
                metaPressed = event.isMetaPressed,
                altPressed = event.isAltPressed,
            ) ?: return super.onKeyDown(keyCode, event)
        // Focused Views receive the event first. Ghostty therefore retains
        // Escape and terminal input, while unhandled app commands arrive here.
        hardwareShortcutSequence += 1
        hardwareShortcut.value = S5HardwareShortcutEvent(hardwareShortcutSequence, shortcut)
        consumedShortcutKeyCode = keyCode
        return true
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        // Key-up belongs to a consumed key-down too; swallowing only key-down
        // lets a bare character leak into the newly focused destination after a
        // modifier is released first.
        if (keyCode == consumedShortcutKeyCode) {
            consumedShortcutKeyCode = null
            return true
        }
        return super.onKeyUp(keyCode, event)
    }
}
