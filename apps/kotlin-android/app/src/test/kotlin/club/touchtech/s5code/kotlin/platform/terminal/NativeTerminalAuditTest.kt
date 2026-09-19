package club.touchtech.s5code.kotlin.platform.terminal

import java.io.File
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Keeps terminal parity on the native Ghostty boundary instead of regressing to a text widget. */
class NativeTerminalAuditTest {
    @Test
    fun `terminal screen is backed by Ghostty and bundled Nerd Fonts`() {
        val appRoot = findAppRoot()
        val screen =
            File(
                    appRoot,
                    "src/main/kotlin/club/touchtech/s5code/kotlin/feature/terminal/TerminalScreen.kt",
                )
                .readText()
        val view =
            File(
                    appRoot,
                    "src/main/kotlin/club/touchtech/s5code/kotlin/platform/terminal/S5TerminalView.kt",
                )
                .readText()
        assertTrue("terminal must host the native VT", "S5TerminalView" in screen)
        assertFalse("raw PTY lines must not be rendered with Compose Text", "current.lines[" in screen)
        assertTrue("hardware keys must use Ghostty's encoder", "nativeEncodeKey(" in view)
        assertTrue(File(appRoot, "src/main/assets/fonts/MesloLGS-NF-Regular.ttf").isFile)
        assertTrue(File(appRoot, "src/main/assets/fonts/MesloLGS-NF-Bold.ttf").isFile)
        listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64").forEach { abi ->
            assertTrue(File(appRoot, "src/main/jniLibs/$abi/libghostty-vt.so").isFile)
        }
    }

    private fun findAppRoot(): File {
        val candidates =
            generateSequence(File(requireNotNull(System.getProperty("user.dir")))) { it.parentFile }
                .flatMap { root ->
                    sequenceOf(File(root, "app"), File(root, "apps/kotlin-android/app"))
                }
        return candidates.firstOrNull { File(it, "src/main").isDirectory }
            ?: error("Could not locate Kotlin Android app root")
    }
}
