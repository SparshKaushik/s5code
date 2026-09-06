package club.touchtech.s5code.kotlin.design.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color

/**
 * Branded expressive fallback used whenever dynamic color is unavailable or the
 * user opts out. Primary carries the S5 blue from the shared brand assets;
 * tertiary supplies the violet accent the other clients use for agent activity.
 */
internal val S5LightColorScheme: ColorScheme =
    lightColorScheme(
        primary = Color(0xFF00639B),
        onPrimary = Color(0xFFFFFFFF),
        primaryContainer = Color(0xFFCEE5FF),
        onPrimaryContainer = Color(0xFF001D33),
        inversePrimary = Color(0xFF97CBFF),
        secondary = Color(0xFF51606F),
        onSecondary = Color(0xFFFFFFFF),
        secondaryContainer = Color(0xFFD4E4F6),
        onSecondaryContainer = Color(0xFF0D1D2A),
        tertiary = Color(0xFF6750A4),
        onTertiary = Color(0xFFFFFFFF),
        tertiaryContainer = Color(0xFFE9DDFF),
        onTertiaryContainer = Color(0xFF22005D),
        error = Color(0xFFBA1A1A),
        onError = Color(0xFFFFFFFF),
        errorContainer = Color(0xFFFFDAD6),
        onErrorContainer = Color(0xFF410002),
        background = Color(0xFFF8F9FF),
        onBackground = Color(0xFF191C20),
        surface = Color(0xFFF8F9FF),
        onSurface = Color(0xFF191C20),
        surfaceVariant = Color(0xFFDDE3EA),
        onSurfaceVariant = Color(0xFF41474D),
        surfaceTint = Color(0xFF00639B),
        outline = Color(0xFF71787E),
        outlineVariant = Color(0xFFC1C7CE),
        surfaceBright = Color(0xFFF8F9FF),
        surfaceDim = Color(0xFFD8DAE0),
        surfaceContainerLowest = Color(0xFFFFFFFF),
        surfaceContainerLow = Color(0xFFF2F3FA),
        surfaceContainer = Color(0xFFECEEF4),
        surfaceContainerHigh = Color(0xFFE6E8EE),
        surfaceContainerHighest = Color(0xFFE1E2E8),
        inverseSurface = Color(0xFF2E3135),
        inverseOnSurface = Color(0xFFEFF1F7),
        scrim = Color(0xFF000000),
        primaryFixed = Color(0xFFCEE5FF),
        onPrimaryFixed = Color(0xFF001D33),
        primaryFixedDim = Color(0xFF97CBFF),
        onPrimaryFixedVariant = Color(0xFF004A76),
        tertiaryFixed = Color(0xFFE9DDFF),
        onTertiaryFixed = Color(0xFF22005D),
        tertiaryFixedDim = Color(0xFFCFBCFF),
        onTertiaryFixedVariant = Color(0xFF4F378A),
    )

internal val S5DarkColorScheme: ColorScheme =
    darkColorScheme(
        primary = Color(0xFF97CBFF),
        onPrimary = Color(0xFF003354),
        primaryContainer = Color(0xFF004A76),
        onPrimaryContainer = Color(0xFFCEE5FF),
        inversePrimary = Color(0xFF00639B),
        secondary = Color(0xFFB8C8DA),
        onSecondary = Color(0xFF233240),
        secondaryContainer = Color(0xFF394857),
        onSecondaryContainer = Color(0xFFD4E4F6),
        tertiary = Color(0xFFCFBCFF),
        onTertiary = Color(0xFF381E72),
        tertiaryContainer = Color(0xFF4F378A),
        onTertiaryContainer = Color(0xFFE9DDFF),
        error = Color(0xFFFFB4AB),
        onError = Color(0xFF690005),
        errorContainer = Color(0xFF93000A),
        onErrorContainer = Color(0xFFFFDAD6),
        background = Color(0xFF101318),
        onBackground = Color(0xFFE1E2E8),
        surface = Color(0xFF101318),
        onSurface = Color(0xFFE1E2E8),
        surfaceVariant = Color(0xFF41474D),
        onSurfaceVariant = Color(0xFFC1C7CE),
        surfaceTint = Color(0xFF97CBFF),
        outline = Color(0xFF8B9198),
        outlineVariant = Color(0xFF41474D),
        surfaceBright = Color(0xFF36393E),
        surfaceDim = Color(0xFF101318),
        surfaceContainerLowest = Color(0xFF0B0E12),
        surfaceContainerLow = Color(0xFF191C20),
        surfaceContainer = Color(0xFF1D2024),
        surfaceContainerHigh = Color(0xFF272A2F),
        surfaceContainerHighest = Color(0xFF32353A),
        inverseSurface = Color(0xFFE1E2E8),
        inverseOnSurface = Color(0xFF2E3135),
        scrim = Color(0xFF000000),
        primaryFixed = Color(0xFFCEE5FF),
        onPrimaryFixed = Color(0xFF001D33),
        primaryFixedDim = Color(0xFF97CBFF),
        onPrimaryFixedVariant = Color(0xFF004A76),
        tertiaryFixed = Color(0xFFE9DDFF),
        onTertiaryFixed = Color(0xFF22005D),
        tertiaryFixedDim = Color(0xFFCFBCFF),
        onTertiaryFixedVariant = Color(0xFF4F378A),
    )

/**
 * Status hues shared with the other S5 Code surfaces (web sidebar, Live
 * Updates, widgets) so a thread reads the same everywhere. These are the only
 * colors outside the M3 scheme; they always ship with a text label so color is
 * never the sole state signal.
 *
 * Each status carries a full M3-shaped role set, because a status is drawn two
 * different ways and the two need different pairs:
 *
 * - `container` + `onContainer` for tonal fills that hold text (badges, pills).
 * - `solid` + `onSolid` for the saturated accent (icon badges, dots, rules) and
 *   anything drawn on top of it.
 *
 * Mixing them is the bug this shape prevents: `onSolid` is white in light mode,
 * and white on a tone-90 pastel container measures about 1.3:1. Every pair here
 * clears 7:1 for label text and 4.5:1 for an accent against the app surfaces.
 */
data class S5StatusColors(
    val approval: Color,
    val onApproval: Color,
    val approvalContainer: Color,
    val onApprovalContainer: Color,
    val input: Color,
    val onInput: Color,
    val inputContainer: Color,
    val onInputContainer: Color,
    val working: Color,
    val onWorking: Color,
    val workingContainer: Color,
    val onWorkingContainer: Color,
    val failed: Color,
    val onFailed: Color,
    val failedContainer: Color,
    val onFailedContainer: Color,
    val settled: Color,
    val onSettled: Color,
    val settledContainer: Color,
    val onSettledContainer: Color,
    val added: Color,
    val removed: Color,
) {
    companion object {
        val Light =
            S5StatusColors(
                approval = Color(0xFF805600),
                onApproval = Color(0xFFFFFFFF),
                approvalContainer = Color(0xFFFFDEA8),
                onApprovalContainer = Color(0xFF2A1800),
                input = Color(0xFF3F3BB5),
                onInput = Color(0xFFFFFFFF),
                inputContainer = Color(0xFFE0E0FF),
                onInputContainer = Color(0xFF191970),
                working = Color(0xFF00658E),
                onWorking = Color(0xFFFFFFFF),
                workingContainer = Color(0xFFC5E7FF),
                onWorkingContainer = Color(0xFF001E2F),
                failed = Color(0xFFB3261E),
                onFailed = Color(0xFFFFFFFF),
                failedContainer = Color(0xFFFFDAD6),
                onFailedContainer = Color(0xFF410002),
                settled = Color(0xFF2E6B34),
                onSettled = Color(0xFFFFFFFF),
                settledContainer = Color(0xFFC4EFC0),
                onSettledContainer = Color(0xFF07220C),
                added = Color(0xFF1B6B3A),
                removed = Color(0xFFB3261E),
            )

        val Dark =
            S5StatusColors(
                approval = Color(0xFFFFD08A),
                onApproval = Color(0xFF452B00),
                approvalContainer = Color(0xFF5E4100),
                onApprovalContainer = Color(0xFFFFDEA8),
                input = Color(0xFFC0C1FF),
                onInput = Color(0xFF1E1A8E),
                inputContainer = Color(0xFF3A34A8),
                onInputContainer = Color(0xFFE0E0FF),
                working = Color(0xFF7FD0FF),
                onWorking = Color(0xFF00344B),
                workingContainer = Color(0xFF00496B),
                onWorkingContainer = Color(0xFFC5E7FF),
                failed = Color(0xFFFFB4AB),
                onFailed = Color(0xFF690005),
                failedContainer = Color(0xFF93000A),
                onFailedContainer = Color(0xFFFFDAD6),
                settled = Color(0xFFA8D3A4),
                onSettled = Color(0xFF07220C),
                settledContainer = Color(0xFF22512A),
                onSettledContainer = Color(0xFFC4EFC0),
                added = Color(0xFF8FD9A5),
                removed = Color(0xFFFFB4AB),
            )
    }
}
