package club.touchtech.s5code.kotlin.app

import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController

/**
 * Independent trailing navigation stack for expanded workspace tools. Chat stays
 * mounted in the center while files, terminal, Git, and review navigate here.
 */
@Composable
internal fun WorkspaceInspectorHost(
    store: AppStore,
    startRoute: String,
    confirmController: S5ConfirmDialogController,
    onDismiss: () -> Unit,
) {
    val navController = rememberNavController()
    val entry by navController.currentBackStackEntryAsState()
    var opened by remember(startRoute) { mutableStateOf(false) }

    // Keep the placeholder below the first inspector route. Its regular top-bar
    // back action can then pop normally; arriving back at the placeholder closes
    // the column rather than showing an empty pane.
    LaunchedEffect(startRoute) { navController.navigate(startRoute) }
    LaunchedEffect(entry?.destination?.route) {
        if (entry?.destination?.route == Routes.WorkspaceEmpty) {
            if (opened) onDismiss()
        } else if (entry != null) {
            opened = true
        }
    }
    BackHandler {
        if (!navController.popBackStack()) onDismiss()
    }
    S5NavGraph(
        navController = navController,
        store = store,
        startDestination = Routes.WorkspaceEmpty,
        widthSizeClass = S5WindowWidth.Expanded,
        confirmController = confirmController,
    )
}
