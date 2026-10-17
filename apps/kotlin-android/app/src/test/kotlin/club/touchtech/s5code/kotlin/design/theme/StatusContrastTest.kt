package club.touchtech.s5code.kotlin.design.theme

import androidx.compose.ui.graphics.Color
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Status badges are a color pair drawn as text on a fill, and the easy mistake is
 * pairing an on-solid color with a tonal container: white on a tone-90 pastel
 * measures about 1.3:1 and looks washed out on every device.
 *
 * These are measurements, not opinions, so they belong in a test rather than a
 * review note. Both palettes are checked in both directions a status is drawn:
 * label on container, and content on the saturated accent.
 */
class StatusContrastTest {

    private fun channel(value: Float): Double {
        val c = value.toDouble()
        return if (c <= 0.03928) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)
    }

    private fun luminance(color: Color): Double =
        0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)

    private fun contrast(a: Color, b: Color): Double {
        val la = luminance(a)
        val lb = luminance(b)
        return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
    }

    /** WCAG AA for normal-size text. Badge labels are small, so this is the floor. */
    private val textMinimum = 4.5

    /** WCAG AA for UI components and graphical objects. */
    private val nonTextMinimum = 3.0

    private fun pairs(colors: S5StatusColors) =
        listOf(
            "approval label" to (colors.onApprovalContainer to colors.approvalContainer),
            "input label" to (colors.onInputContainer to colors.inputContainer),
            "working label" to (colors.onWorkingContainer to colors.workingContainer),
            "failed label" to (colors.onFailedContainer to colors.failedContainer),
            "settled label" to (colors.onSettledContainer to colors.settledContainer),
            "approval on accent" to (colors.onApproval to colors.approval),
            "input on accent" to (colors.onInput to colors.input),
            "working on accent" to (colors.onWorking to colors.working),
            "failed on accent" to (colors.onFailed to colors.failed),
            "settled on accent" to (colors.onSettled to colors.settled),
        )

    @Test
    fun `light status pairs clear AA for text`() {
        pairs(S5StatusColors.Light).forEach { (name, pair) ->
            val ratio = contrast(pair.first, pair.second)
            assertTrue("$name is $ratio:1, below $textMinimum:1", ratio >= textMinimum)
        }
    }

    @Test
    fun `dark status pairs clear AA for text`() {
        pairs(S5StatusColors.Dark).forEach { (name, pair) ->
            val ratio = contrast(pair.first, pair.second)
            assertTrue("$name is $ratio:1, below $textMinimum:1", ratio >= textMinimum)
        }
    }

    @Test
    fun `accents stay visible against the surfaces they are drawn on`() {
        // Rails, dots, and diff counts are graphical, so they only owe 3:1, but
        // they are drawn on both the page surface and a raised container.
        val light = S5StatusColors.Light
        val dark = S5StatusColors.Dark
        val lightSurfaces = listOf(Color(0xFFF8F9FF), Color(0xFFE6E8EE))
        val darkSurfaces = listOf(Color(0xFF101318), Color(0xFF272A2F))

        val lightAccents =
            mapOf(
                "approval" to light.approval,
                "input" to light.input,
                "working" to light.working,
                "failed" to light.failed,
                "settled" to light.settled,
                "added" to light.added,
                "removed" to light.removed,
            )
        val darkAccents =
            mapOf(
                "approval" to dark.approval,
                "input" to dark.input,
                "working" to dark.working,
                "failed" to dark.failed,
                "settled" to dark.settled,
                "added" to dark.added,
                "removed" to dark.removed,
            )

        lightAccents.forEach { (name, color) ->
            lightSurfaces.forEach { surface ->
                val ratio = contrast(color, surface)
                assertTrue("light $name is $ratio:1, below $nonTextMinimum:1", ratio >= nonTextMinimum)
            }
        }
        darkAccents.forEach { (name, color) ->
            darkSurfaces.forEach { surface ->
                val ratio = contrast(color, surface)
                assertTrue("dark $name is $ratio:1, below $nonTextMinimum:1", ratio >= nonTextMinimum)
            }
        }
    }
}
