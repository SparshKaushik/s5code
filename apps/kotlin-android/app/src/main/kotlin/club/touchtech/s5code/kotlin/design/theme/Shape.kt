package club.touchtech.s5code.kotlin.design.theme

import androidx.compose.foundation.shape.CornerSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.Shapes
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp
import androidx.graphics.shapes.RoundedPolygon

/**
 * The complete expanded expressive shape scale, including the "increased" and
 * "extra extra large" sizes M3 Expressive adds on top of the baseline five.
 * Features read shapes from here (or from `MaterialTheme.shapes`), never from
 * hard-coded corner radii.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
internal val S5Shapes: Shapes =
    Shapes(
        extraSmall = RoundedCornerShape(4.dp),
        small = RoundedCornerShape(8.dp),
        medium = RoundedCornerShape(12.dp),
        large = RoundedCornerShape(16.dp),
        largeIncreased = RoundedCornerShape(20.dp),
        extraLarge = RoundedCornerShape(28.dp),
        extraLargeIncreased = RoundedCornerShape(32.dp),
        extraExtraLarge = RoundedCornerShape(48.dp),
    )

/** Fully rounded container used for pills, chips-like affordances, and avatars. */
val S5PillShape: Shape = RoundedCornerShape(CornerSize(percent = 50))

/**
 * Named registry of every [MaterialShapes] entry available in the pinned Compose
 * Material 3 version. Features pick entries from it by name so shape choices stay
 * reviewable rather than scattered as inline `MaterialShapes.X` references.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
object S5MaterialShapes {
    data class Entry(val name: String, val polygon: RoundedPolygon)

    val all: List<Entry> =
        listOf(
            Entry("Circle", MaterialShapes.Circle),
            Entry("Square", MaterialShapes.Square),
            Entry("Slanted", MaterialShapes.Slanted),
            Entry("Arch", MaterialShapes.Arch),
            Entry("Fan", MaterialShapes.Fan),
            Entry("Arrow", MaterialShapes.Arrow),
            Entry("Semi-circle", MaterialShapes.SemiCircle),
            Entry("Oval", MaterialShapes.Oval),
            Entry("Pill", MaterialShapes.Pill),
            Entry("Triangle", MaterialShapes.Triangle),
            Entry("Diamond", MaterialShapes.Diamond),
            Entry("Clam shell", MaterialShapes.ClamShell),
            Entry("Pentagon", MaterialShapes.Pentagon),
            Entry("Gem", MaterialShapes.Gem),
            Entry("Sunny", MaterialShapes.Sunny),
            Entry("Very sunny", MaterialShapes.VerySunny),
            Entry("Cookie 4-sided", MaterialShapes.Cookie4Sided),
            Entry("Cookie 6-sided", MaterialShapes.Cookie6Sided),
            Entry("Cookie 7-sided", MaterialShapes.Cookie7Sided),
            Entry("Cookie 9-sided", MaterialShapes.Cookie9Sided),
            Entry("Cookie 12-sided", MaterialShapes.Cookie12Sided),
            Entry("Ghost-ish", MaterialShapes.Ghostish),
            Entry("Clover 4-leaf", MaterialShapes.Clover4Leaf),
            Entry("Clover 8-leaf", MaterialShapes.Clover8Leaf),
            Entry("Burst", MaterialShapes.Burst),
            Entry("Soft burst", MaterialShapes.SoftBurst),
            Entry("Boom", MaterialShapes.Boom),
            Entry("Soft boom", MaterialShapes.SoftBoom),
            Entry("Flower", MaterialShapes.Flower),
            Entry("Puffy", MaterialShapes.Puffy),
            Entry("Puffy diamond", MaterialShapes.PuffyDiamond),
            Entry("Pixel circle", MaterialShapes.PixelCircle),
            Entry("Pixel triangle", MaterialShapes.PixelTriangle),
            Entry("Bun", MaterialShapes.Bun),
            Entry("Heart", MaterialShapes.Heart),
        )

    /** Iconic shape for provider/agent avatars. */
    @Composable fun avatar(): Shape = MaterialShapes.Cookie9Sided.toShape()

    /** Iconic shape for the pending-approval hero moment. */
    @Composable fun approval(): Shape = MaterialShapes.Burst.toShape()

    /** Iconic shape for the awaiting-input hero moment. */
    @Composable fun input(): Shape = MaterialShapes.Clover4Leaf.toShape()

    /** Iconic shape for active agent work. */
    @Composable fun working(): Shape = MaterialShapes.SoftBurst.toShape()

    /** Iconic shape for a settled/finished thread. */
    @Composable fun settled(): Shape = MaterialShapes.Pill.toShape()

    /** Iconic shape for failed turns. */
    @Composable fun failed(): Shape = MaterialShapes.Diamond.toShape()

    /** Iconic shape used behind empty-state and onboarding hero art. */
    @Composable fun hero(): Shape = MaterialShapes.Flower.toShape()
}
