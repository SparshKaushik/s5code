package club.touchtech.s5code.kotlin.design.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color

/**
 * The named color themes shared with the web and React Native clients.
 *
 * Ported from `packages/shared/src/themePalettes.ts`: the role names and the
 * palette values are the same file's, converted from oklch to sRGB with the
 * math in `themeColorToNativeColor` so they can be diffed by eye. Regenerate
 * rather than hand-editing when the shared palettes change.
 */

/** The roles a theme must supply; anything else derives from the M3 defaults. */
data class S5ThemePalette(
    val canvas: Long,
    val surface: Long,
    val surfaceRaised: Long,
    val surfaceOverlay: Long,
    val text: Long,
    val textMuted: Long,
    val border: Long,
    val accent: Long,
    val accentForeground: Long,
    val secondary: Long,
    val secondaryForeground: Long,
    val muted: Long,
    val mutedForeground: Long,
    val error: Long,
    val errorForeground: Long,
    val errorSurface: Long,
    val accentSurface: Long,
    val accentSurfaceForeground: Long,
    val messageSurface: Long,
    val messageForeground: Long,
    val messageAction: Long,
    val codeBackground: Long,
    val codeForeground: Long,
)

/** A named theme carries both appearances, so the picker's choice is stable. */
data class S5NamedTheme(val id: String, val label: String, val light: S5ThemePalette, val dark: S5ThemePalette)

private val T3_CHAT_LIGHT =
    S5ThemePalette(
        canvas = 0xFFFDF7FD,
        surface = 0xFFFAF3FB,
        surfaceRaised = 0xFFFDFAFD,
        surfaceOverlay = 0xFFFFFFFF,
        text = 0xFF501854,
        textMuted = 0xFFAC1668,
        border = 0xFFEEE1ED,
        accent = 0xFFDB2777,
        accentForeground = 0xFFFFFFFF,
        secondary = 0xFFF1C4E6,
        secondaryForeground = 0xFF77347C,
        muted = 0xFFEAA7CB,
        mutedForeground = 0xFF8D1255,
        error = 0xFFF7086C,
        errorForeground = 0xFF9D174D,
        errorSurface = 0xFFFDE4F1,
        accentSurface = 0xFFF3E6F5,
        accentSurfaceForeground = 0xFF454554,
        messageSurface = 0xFFF7DEF2,
        messageForeground = 0xFF492C61,
        messageAction = 0xFFDB2777,
        codeBackground = 0xFFF5ECF9,
        codeForeground = 0xFF673C8B,
    )

private val T3_CHAT_DARK =
    S5ThemePalette(
        canvas = 0xFF1F1A24,
        surface = 0xFF29232D,
        surfaceRaised = 0xFF2C2631,
        surfaceOverlay = 0xFF100A0E,
        text = 0xFFF9F8FB,
        textMuted = 0xFFE7D0DD,
        border = 0xFF27242C,
        accent = 0xFFA3004C,
        accentForeground = 0xFFFBD0E8,
        secondary = 0xFF362D3D,
        secondaryForeground = 0xFFD4C7E1,
        muted = 0xFF423A45,
        mutedForeground = 0xFFE7D0DD,
        error = 0xFF9D174D,
        errorForeground = 0xFFFBD0E8,
        errorSurface = 0xFF331A2B,
        accentSurface = 0xFF463753,
        accentSurfaceForeground = 0xFFF8F1F5,
        messageSurface = 0xFF2B2431,
        messageForeground = 0xFFF2EBFA,
        messageAction = 0xFFA3004C,
        codeBackground = 0xFF1F1A24,
        codeForeground = 0xFFD8C3EF,
    )

private val GROVE_LIGHT =
    S5ThemePalette(
        canvas = 0xFFF3F7F4,
        surface = 0xFFF3F7F4,
        surfaceRaised = 0xFFECEFED,
        surfaceOverlay = 0xFFE7E9E8,
        text = 0xFF241523,
        textMuted = 0xFF746C73,
        border = 0xFFCBD5D1,
        accent = 0xFF1B7D50,
        accentForeground = 0xFFFFFAFF,
        secondary = 0xFFE2EDE7,
        secondaryForeground = 0xFF241523,
        muted = 0xFFE6F0EA,
        mutedForeground = 0xFF6E696F,
        error = 0xFFFB2C36,
        errorForeground = 0xFFC10007,
        errorSurface = 0xFFF4E7E5,
        accentSurface = 0xFFD5E6DD,
        accentSurfaceForeground = 0xFF241523,
        messageSurface = 0xFFCCE1D7,
        messageForeground = 0xFF241523,
        messageAction = 0xFF8F6410,
        codeBackground = 0xFFEEF1EF,
        codeForeground = 0xFF241523,
    )

private val GROVE_DARK =
    S5ThemePalette(
        canvas = 0xFF1B2821,
        surface = 0xFF1B2821,
        surfaceRaised = 0xFF36413C,
        surfaceOverlay = 0xFF444D49,
        text = 0xFFFFFAFF,
        textMuted = 0xFF919595,
        border = 0xFF415F4F,
        accent = 0xFF69D69A,
        accentForeground = 0xFF241523,
        secondary = 0xFF2A4B39,
        secondaryForeground = 0xFFFFFAFF,
        muted = 0xFF253E31,
        mutedForeground = 0xFF9DA5A2,
        error = 0xFFFB414A,
        errorForeground = 0xFFFF6668,
        errorSurface = 0xFF3F2C28,
        accentSurface = 0xFF325C46,
        accentSurfaceForeground = 0xFFFFFAFF,
        messageSurface = 0xFF37664D,
        messageForeground = 0xFFFFFAFF,
        messageAction = 0xFFE3B34E,
        codeBackground = 0xFF28342E,
        codeForeground = 0xFFFFFAFF,
    )

private val OCEAN_LIGHT =
    S5ThemePalette(
        canvas = 0xFFF5F7F8,
        surface = 0xFFF5F7F8,
        surfaceRaised = 0xFFEDEFF1,
        surfaceOverlay = 0xFFE8E9EB,
        text = 0xFF241523,
        textMuted = 0xFF746C75,
        border = 0xFFCDD4DC,
        accent = 0xFF2672AF,
        accentForeground = 0xFFFFFAFF,
        secondary = 0xFFE4ECF2,
        secondaryForeground = 0xFF241523,
        muted = 0xFFE8EFF4,
        mutedForeground = 0xFF6F6873,
        error = 0xFFFB2C36,
        errorForeground = 0xFFC10007,
        errorSurface = 0xFFF5E6E9,
        accentSurface = 0xFFD8E4EE,
        accentSurfaceForeground = 0xFF241523,
        messageSurface = 0xFFD0DFEB,
        messageForeground = 0xFF241523,
        messageAction = 0xFF0A6F75,
        codeBackground = 0xFFF0F1F3,
        codeForeground = 0xFF241523,
    )

private val OCEAN_DARK =
    S5ThemePalette(
        canvas = 0xFF17212B,
        surface = 0xFF17212B,
        surfaceRaised = 0xFF333B45,
        surfaceOverlay = 0xFF414851,
        text = 0xFFFFFAFF,
        textMuted = 0xFF8D8F97,
        border = 0xFF405567,
        accent = 0xFF70B9EE,
        accentForeground = 0xFF241523,
        secondary = 0xFF293F52,
        secondaryForeground = 0xFFFFFAFF,
        muted = 0xFF233544,
        mutedForeground = 0xFF969CA6,
        error = 0xFFFB414A,
        errorForeground = 0xFFFF6467,
        errorSurface = 0xFF3C2630,
        accentSurface = 0xFF324E66,
        accentSurfaceForeground = 0xFFFFFAFF,
        messageSurface = 0xFF375871,
        messageForeground = 0xFFFFFAFF,
        messageAction = 0xFF5BD0D6,
        codeBackground = 0xFF252E38,
        codeForeground = 0xFFFFFAFF,
    )

private val EMBER_LIGHT =
    S5ThemePalette(
        canvas = 0xFFF9F7F5,
        surface = 0xFFF9F7F5,
        surfaceRaised = 0xFFF1EFEE,
        surfaceOverlay = 0xFFECE9E9,
        text = 0xFF241523,
        textMuted = 0xFF766C74,
        border = 0xFFDDD2CE,
        accent = 0xFFAE552A,
        accentForeground = 0xFFFFFAFF,
        secondary = 0xFFF3EAE5,
        secondaryForeground = 0xFF241523,
        muted = 0xFFF4EDE9,
        mutedForeground = 0xFF74686F,
        error = 0xFFFB2C36,
        errorForeground = 0xFFC10007,
        errorSurface = 0xFFF9E7E6,
        accentSurface = 0xFFEEE0D9,
        accentSurfaceForeground = 0xFF241523,
        messageSurface = 0xFFEBDAD1,
        messageForeground = 0xFF241523,
        messageAction = 0xFFB23535,
        codeBackground = 0xFFF3F1F0,
        codeForeground = 0xFF241523,
    )

private val EMBER_DARK =
    S5ThemePalette(
        canvas = 0xFF291E1A,
        surface = 0xFF291E1A,
        surfaceRaised = 0xFF433835,
        surfaceOverlay = 0xFF4F4543,
        text = 0xFFFFFAFF,
        textMuted = 0xFF968E8F,
        border = 0xFF664C3F,
        accent = 0xFFF09A64,
        accentForeground = 0xFF241523,
        secondary = 0xFF513728,
        secondaryForeground = 0xFFFFFAFF,
        muted = 0xFF432E23,
        mutedForeground = 0xFFA59996,
        error = 0xFFFB414A,
        errorForeground = 0xFFFF6467,
        errorSurface = 0xFF4A2321,
        accentSurface = 0xFF644330,
        accentSurfaceForeground = 0xFFFFFAFF,
        messageSurface = 0xFF704B34,
        messageForeground = 0xFFFFFAFF,
        messageAction = 0xFFF78A7A,
        codeBackground = 0xFF362B27,
        codeForeground = 0xFFFFFAFF,
    )

private val IRIS_LIGHT =
    S5ThemePalette(
        canvas = 0xFFF8F7F9,
        surface = 0xFFF8F7F9,
        surfaceRaised = 0xFFF0EFF2,
        surfaceOverlay = 0xFFEBE9ED,
        text = 0xFF241523,
        textMuted = 0xFF766C76,
        border = 0xFFD6D1DE,
        accent = 0xFF7253B9,
        accentForeground = 0xFFFFFAFF,
        secondary = 0xFFEDEAF4,
        secondaryForeground = 0xFF241523,
        muted = 0xFFF0EDF6,
        mutedForeground = 0xFF726874,
        error = 0xFFFB2C36,
        errorForeground = 0xFFC10007,
        errorSurface = 0xFFF8E6EA,
        accentSurface = 0xFFE5E0F0,
        accentSurfaceForeground = 0xFF241523,
        messageSurface = 0xFFE0D9EE,
        messageForeground = 0xFF241523,
        messageAction = 0xFFA82C87,
        codeBackground = 0xFFF2F1F4,
        codeForeground = 0xFF241523,
    )

private val IRIS_DARK =
    S5ThemePalette(
        canvas = 0xFF1D1929,
        surface = 0xFF1D1929,
        surfaceRaised = 0xFF383443,
        surfaceOverlay = 0xFF454250,
        text = 0xFFFFFAFF,
        textMuted = 0xFF8E8A95,
        border = 0xFF4D4366,
        accent = 0xFF9D7DF2,
        accentForeground = 0xFF241523,
        secondary = 0xFF362D51,
        secondaryForeground = 0xFFFFFAFF,
        muted = 0xFF2D2643,
        mutedForeground = 0xFF9690A1,
        error = 0xFFFB414A,
        errorForeground = 0xFFFF6467,
        errorSurface = 0xFF40202E,
        accentSurface = 0xFF433765,
        accentSurfaceForeground = 0xFFFFFAFF,
        messageSurface = 0xFF4B3D72,
        messageForeground = 0xFFFFFAFF,
        messageAction = 0xFFF099D8,
        codeBackground = 0xFF2A2736,
        codeForeground = 0xFFFFFAFF,
    )

/** Named themes in picker order, matching `BUILT_IN_THEMES`. */
internal val S5_NAMED_THEMES =
    listOf(
        S5NamedTheme("t3-chat", "T3 Chat", T3_CHAT_LIGHT, T3_CHAT_DARK),
        S5NamedTheme("grove", "Grove", GROVE_LIGHT, GROVE_DARK),
        S5NamedTheme("ocean", "Ocean", OCEAN_LIGHT, OCEAN_DARK),
        S5NamedTheme("ember", "Ember", EMBER_LIGHT, EMBER_DARK),
        S5NamedTheme("iris", "Iris", IRIS_LIGHT, IRIS_DARK),
    )

/**
 * The M3 scheme for one palette.
 *
 * The role mapping is semantic rather than tonal: the shared palettes do not
 * carry a ten-rung surface ladder, so the container rungs take the same answer
 * the RN tokens take — `surface`/`surfaceRaised`/`secondary`/`muted` are the
 * escalating fills, and `canvas`/`overlay` anchor the ends.
 */
internal fun S5ThemePalette.toColorScheme(dark: Boolean): ColorScheme {
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = Color(accent),
        onPrimary = Color(accentForeground),
        primaryContainer = Color(accentSurface),
        onPrimaryContainer = Color(accentSurfaceForeground),
        inversePrimary = Color(accent),
        secondary = Color(mutedForeground),
        onSecondary = Color(text),
        secondaryContainer = Color(secondary),
        onSecondaryContainer = Color(secondaryForeground),
        tertiary = Color(messageAction),
        onTertiary = Color(accentForeground),
        tertiaryContainer = Color(messageSurface),
        onTertiaryContainer = Color(messageForeground),
        error = Color(error),
        // `onError` sits on the saturated role, so it wants the neutral end of
        // the palette, not `errorForeground` (which is authored for `errorSurface`).
        onError = Color(canvas),
        errorContainer = Color(errorSurface),
        onErrorContainer = Color(errorForeground),
        background = Color(canvas),
        onBackground = Color(text),
        surface = Color(canvas),
        onSurface = Color(text),
        surfaceVariant = Color(muted),
        onSurfaceVariant = Color(mutedForeground),
        surfaceTint = Color(accent),
        outline = Color(mutedForeground),
        outlineVariant = Color(border),
        surfaceDim = Color(if (dark) surfaceOverlay else muted),
        surfaceBright = Color(if (dark) secondary else surfaceOverlay),
        // The shared palettes carry no ten-rung surface ladder, so the container
        // roles take the same escalation the other clients give their tokens:
        // `surfaceOverlay` is the extreme, `surface` is the recessed card
        // (`--color-card-alt`), `surfaceRaised` the card (`--color-card`), and
        // `secondary`/`muted` the raised fills. Grove, Ocean, Ember, and Iris
        // collapse `canvas` and `surface` onto one value, so anything that
        // mapped the card onto `surface`/`canvas` rendered invisible on the
        // screen fill — the highlight that vanished when a named theme loaded.
        surfaceContainerLowest = Color(surfaceOverlay),
        surfaceContainerLow = Color(surface),
        surfaceContainer = Color(surfaceRaised),
        surfaceContainerHigh = Color(secondary),
        surfaceContainerHighest = Color(muted),
        inverseSurface = Color(muted),
        inverseOnSurface = Color(mutedForeground),
        scrim = Color(0xFF000000),
        primaryFixed = Color(accentSurface),
        onPrimaryFixed = Color(accentSurfaceForeground),
        primaryFixedDim = Color(accent),
        onPrimaryFixedVariant = Color(accent),
        tertiaryFixed = Color(messageSurface),
        onTertiaryFixed = Color(messageForeground),
        tertiaryFixedDim = Color(messageAction),
        onTertiaryFixedVariant = Color(messageForeground),
    )
}
