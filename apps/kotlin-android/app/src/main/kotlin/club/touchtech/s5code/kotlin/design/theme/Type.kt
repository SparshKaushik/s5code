package club.touchtech.s5code.kotlin.design.theme

import androidx.compose.material3.Typography
import androidx.compose.runtime.Immutable
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * Baseline type comes straight from Material 3, which already carries both the
 * standard and the expressive `*Emphasized` roles in the pinned version. Only
 * technical content deviates, and it does so through [S5CodeType] so monospace
 * never leaks into prose.
 */
internal val S5Typography: Typography = Typography()

/**
 * Monospaced roles for code, diffs, paths, logs, and terminal content. Sizes are
 * expressed in sp so they scale with the user's font setting, and line heights
 * leave room for descenders at large scales.
 */
@Immutable
data class S5CodeType(
    val code: TextStyle =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontSize = 13.sp,
            lineHeight = 20.sp,
        ),
    val codeSmall: TextStyle =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontSize = 11.sp,
            lineHeight = 16.sp,
        ),
    val codeEmphasized: TextStyle =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
            lineHeight = 20.sp,
        ),
    val terminal: TextStyle =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontSize = 12.sp,
            lineHeight = 17.sp,
        ),
    /** Branch names, paths, and PR numbers inside otherwise proportional rows. */
    val inlineTechnical: TextStyle =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontSize = 11.sp,
            lineHeight = 15.sp,
        ),
)
