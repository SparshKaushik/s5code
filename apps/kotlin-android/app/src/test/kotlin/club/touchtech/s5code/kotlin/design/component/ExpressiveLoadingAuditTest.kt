package club.touchtech.s5code.kotlin.design.component

import java.io.File
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Guards the design-system boundary for visible loading indicators. */
class ExpressiveLoadingAuditTest {
    @Test
    fun `loading indicator implementations stay in the expressive design layer`() {
        val sourceRoot = findSourceRoot()
        val kotlinFiles = sourceRoot.walkTopDown().filter { it.extension == "kt" }.toList()
        val forbidden = listOf("CircularProgressIndicator(", "rememberInfiniteTransition(")

        kotlinFiles.forEach { file ->
            val relative = file.relativeTo(sourceRoot).invariantSeparatorsPath
            val source = file.readText()
            forbidden.forEach { call ->
                assertFalse("$relative must not use $call for loading", call in source)
            }
            if (!relative.startsWith("design/component/")) {
                assertFalse(
                    "$relative must use an S5 loading wrapper",
                    Regex("(?<!S5)(?<!Contained)(?<!Wavy)LoadingIndicator\\(").containsMatchIn(source),
                )
            }
        }

        val strip = File(sourceRoot, "design/component/LoadingStrip.kt").readText()
        val action = File(sourceRoot, "design/component/ActionProgressOverlay.kt").readText()
        val refresh = File(sourceRoot, "design/component/PullToRefresh.kt").readText()
        assertTrue("top-edge loading must use Material 3 Expressive waves", "LinearWavyProgressIndicator(" in strip)
        assertTrue("action progress must use Material 3 Expressive loading", "LoadingIndicator(" in action)
        assertTrue("pull refresh must use its expressive loading indicator", "PullToRefreshDefaults.LoadingIndicator(" in refresh)
    }

    private fun findSourceRoot(): File {
        val candidates =
            generateSequence(File(requireNotNull(System.getProperty("user.dir")))) { it.parentFile }
                .flatMap { root ->
                    sequenceOf(
                        File(root, "app/src/main/kotlin/club/touchtech/s5code/kotlin"),
                        File(root, "apps/kotlin-android/app/src/main/kotlin/club/touchtech/s5code/kotlin"),
                    )
                }
        return candidates.firstOrNull(File::isDirectory)
            ?: error("Could not locate Kotlin Android source root")
    }
}
