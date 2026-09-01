package club.touchtech.s5code.kotlin.design.theme

import android.os.Build
import android.provider.Settings
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** User-selectable theme mode, mirroring the RN client's appearance setting. */
enum class S5ThemeMode {
    System,
    Light,
    Dark,
}

/** Semantic spacing scale. Features never hard-code raw dp for layout rhythm. */
data class S5Spacing(
    val hair: Dp = 2.dp,
    val tiny: Dp = 4.dp,
    val small: Dp = 8.dp,
    val medium: Dp = 12.dp,
    val large: Dp = 16.dp,
    val xLarge: Dp = 20.dp,
    val xxLarge: Dp = 24.dp,
    val section: Dp = 32.dp,
    /** Horizontal screen gutter shared by every scrolling surface. */
    val gutter: Dp = 16.dp,
)

val LocalS5Spacing: ProvidableCompositionLocal<S5Spacing> = staticCompositionLocalOf { S5Spacing() }
val LocalS5StatusColors: ProvidableCompositionLocal<S5StatusColors> =
    staticCompositionLocalOf { S5StatusColors.Light }
val LocalS5CodeType: ProvidableCompositionLocal<S5CodeType> = staticCompositionLocalOf { S5CodeType() }

/**
 * True when the system animator duration scale is 0 (Developer options or an
 * accessibility preference). Screens use this to drop non-essential transforms
 * and shape morphs while keeping immediate state feedback.
 */
val LocalS5ReducedMotion: ProvidableCompositionLocal<Boolean> = staticCompositionLocalOf { false }

/**
 * The one theme wrapper every S5 Code destination is composed inside. It wires
 * [MaterialExpressiveTheme] to the expanded shape scale, the expressive motion
 * scheme, and either dynamic color or the branded fallback.
 *
 * Experimental expressive APIs stay opted-in here and in `design/`; feature code
 * consumes only stable S5 wrappers.
 */
@Composable
fun S5Theme(
    themeMode: S5ThemeMode = S5ThemeMode.System,
    dynamicColor: Boolean = true,
    content: @Composable () -> Unit,
) {
    val dark =
        when (themeMode) {
            S5ThemeMode.System -> isSystemInDarkTheme()
            S5ThemeMode.Light -> false
            S5ThemeMode.Dark -> true
        }
    val context = LocalContext.current
    val supportsDynamicColor = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val colorScheme =
        when {
            dynamicColor && supportsDynamicColor && dark -> dynamicDarkColorScheme(context)
            dynamicColor && supportsDynamicColor -> dynamicLightColorScheme(context)
            dark -> S5DarkColorScheme
            else -> S5LightColorScheme
        }
    val inspection = LocalInspectionMode.current
    val reducedMotion =
        remember(context, inspection) {
            if (inspection) {
                false
            } else {
                Settings.Global.getFloat(
                    context.contentResolver,
                    Settings.Global.ANIMATOR_DURATION_SCALE,
                    1f,
                ) == 0f
            }
        }

    CompositionLocalProvider(
        LocalS5Spacing provides S5Spacing(),
        LocalS5CodeType provides S5CodeType(),
        LocalS5StatusColors provides if (dark) S5StatusColors.Dark else S5StatusColors.Light,
        LocalS5ReducedMotion provides reducedMotion,
    ) {
        MaterialExpressiveTheme(
            colorScheme = colorScheme,
            motionScheme = if (reducedMotion) MotionScheme.standard() else MotionScheme.expressive(),
            shapes = S5Shapes,
            typography = S5Typography,
            content = content,
        )
    }
}

/** Convenience accessors so feature code reads one consistent surface. */
object S5Theme {
    val spacing: S5Spacing
        @Composable get() = LocalS5Spacing.current

    val status: S5StatusColors
        @Composable get() = LocalS5StatusColors.current

    val code: S5CodeType
        @Composable get() = LocalS5CodeType.current

    val reducedMotion: Boolean
        @Composable get() = LocalS5ReducedMotion.current

    val shapes
        @Composable get() = MaterialTheme.shapes
}
