package club.touchtech.s5code.kotlin.design.component

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.semantics

/**
 * Material 3 Expressive non-blocking progress along a screen's top edge.
 *
 * Both determinate and indeterminate states use the themed wavy indicator rather
 * than a hand-animated rectangle, so motion, wavelength, stroke, and track come
 * from the pinned Material 3 Expressive implementation.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun S5LoadingStrip(
    modifier: Modifier = Modifier,
    progress: Float? = null,
    color: Color = MaterialTheme.colorScheme.primary,
) {
    val hidden = modifier.fillMaxWidth().semantics { hideFromAccessibility() }
    if (progress == null) {
        LinearWavyProgressIndicator(
            modifier = hidden,
            color = color,
        )
    } else {
        LinearWavyProgressIndicator(
            progress = { progress.coerceIn(0f, 1f) },
            modifier = hidden,
            color = color,
        )
    }
}
