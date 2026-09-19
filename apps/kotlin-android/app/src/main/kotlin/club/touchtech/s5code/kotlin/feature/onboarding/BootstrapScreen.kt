package club.touchtech.s5code.kotlin.feature.onboarding

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Terminal
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.design.component.S5ShapeBadge
import club.touchtech.s5code.kotlin.design.theme.S5MaterialShapes
import club.touchtech.s5code.kotlin.design.theme.S5Theme

/**
 * Cold-start gate.
 *
 * The app cannot know where a launch belongs until the stored session has been
 * read, and guessing shows the wrong screen for a frame: onboarding's hero on a
 * paired device, or an empty home on an unpaired one. This destination owns that
 * single frame, then routes once and removes itself from the back stack, so
 * pressing back from the first real screen leaves the app instead of returning
 * to a splash.
 *
 * It is deliberately static — a brand mark and a line of text, no spinner. A
 * restore that finishes in a frame does not need a progress indicator, and an
 * indicator that flashes for 16ms is worse than none.
 */
@Composable
fun BootstrapScreen(store: AppStore, onPaired: () -> Unit, onUnpaired: () -> Unit) {
    val restored by store.sessionRestored.collectAsStateWithLifecycle()
    val paired by store.paired.collectAsStateWithLifecycle()

    LaunchedEffect(restored, paired) {
        if (restored) {
            if (paired) onPaired() else onUnpaired()
        }
    }

    Column(
        Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        S5ShapeBadge(
            icon = Icons.Rounded.Terminal,
            contentDescription = null,
            shape = S5MaterialShapes.hero(),
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            size = 96.dp,
            iconSize = 40.dp,
        )
        Text(
            "S5 Code",
            style = MaterialTheme.typography.headlineSmallEmphasized,
            modifier = Modifier.padding(top = S5Theme.spacing.large),
        )
    }
}
