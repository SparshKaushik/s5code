package club.touchtech.s5code.kotlin.app

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavHostController
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import club.touchtech.s5code.kotlin.design.component.S5ActionProgressOverlay
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogHost
import club.touchtech.s5code.kotlin.design.component.S5GlobalErrorBanner
import club.touchtech.s5code.kotlin.design.component.S5WorkspacePaneDivider
import club.touchtech.s5code.kotlin.design.component.WORKSPACE_DIVIDER_TOUCH_WIDTH
import club.touchtech.s5code.kotlin.design.component.rememberS5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.feature.home.HomeScreen
import club.touchtech.s5code.kotlin.platform.publishShortcuts

/**
 * App root. Owns the theme, one [AppStore], global keyboard actions, and the
 * adaptive navigation shell.
 */
@Composable
fun S5App(
    widthSizeClass: S5WindowWidth,
    pendingLink: DeepLink? = null,
    onLinkConsumed: () -> Unit = {},
    hardwareShortcut: S5HardwareShortcutEvent? = null,
) {
    val store: AppStore = viewModel()
    val preferences by store.preferences.collectAsStateWithLifecycle()
    val paired by store.paired.collectAsStateWithLifecycle()
    val restored by store.sessionRestored.collectAsStateWithLifecycle()
    val recents by store.recentThreads.collectAsStateWithLifecycle()
    val actionProgress by store.actionProgress.collectAsStateWithLifecycle()
    val globalError by store.globalError.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val currentStore by androidx.compose.runtime.rememberUpdatedState(store)

    // Android may freeze the process long enough for a NAT, tunnel, or server to
    // discard its WebSocket without delivering a close callback. The first
    // foreground event therefore replaces every session immediately instead of
    // waiting for a stale keepalive timeout. Ignore the initial ON_START: sessions
    // are already starting from the restored environment list on cold launch.
    androidx.compose.runtime.DisposableEffect(lifecycleOwner) {
        val refreshGate = ForegroundRefreshGate()
        val observer = LifecycleEventObserver { _, event ->
            if (refreshGate.onEvent(event)) currentStore.refreshConnections()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    LaunchedEffect(pendingLink) {
        pendingLink?.let {
            store.openDeepLink(it)
            onLinkConsumed()
        }
    }
    LaunchedEffect(recents) { publishShortcuts(context, recents) }

    S5Theme(themeMode = preferences.themeMode, dynamicColor = preferences.dynamicColor) {
        val navController = rememberNavController()
        val currentEntry by navController.currentBackStackEntryAsState()
        val queued by store.pendingLink.collectAsStateWithLifecycle()
        val confirmController = rememberS5ConfirmDialogController()
        val expanded = widthSizeClass == S5WindowWidth.Expanded && paired
        var closeExpandedInspector by remember { mutableStateOf<(() -> Unit)?>(null) }

        LaunchedEffect(queued, restored, paired) {
            val link = queued ?: return@LaunchedEffect
            if (!restored) return@LaunchedEffect
            if (!paired) {
                store.consumePendingLink()
                return@LaunchedEffect
            }
            navController.navigate(link.route) {
                popUpTo(Routes.Home)
                launchSingleTop = true
            }
            store.consumePendingLink()
        }

        fun handleEscape() {
            val closeInspector = closeExpandedInspector
            if (closeInspector != null) {
                closeInspector()
            } else {
                navController.popBackStack()
            }
        }

        // New task is always global. Search lands on Home on a compact window;
        // the Home instance handles focus once it composes. Expanded Home is
        // persistent, so no navigation is needed there.
        LaunchedEffect(hardwareShortcut?.id) {
            when (hardwareShortcut?.shortcut) {
                S5HardwareShortcut.NewTask -> navController.navigate(Routes.NewTask)
                S5HardwareShortcut.FocusSearch -> {
                    if (!expanded && currentEntry?.destination?.route != Routes.Home) {
                        navController.navigate(Routes.Home) { launchSingleTop = true }
                    }
                }
                S5HardwareShortcut.Escape -> {
                    // Persistent Home gets first refusal so Escape can close its
                    // focused search field before changing workspace navigation.
                    if (!expanded && currentEntry?.destination?.route != Routes.Home) {
                        handleEscape()
                    }
                }
                null -> Unit
            }
        }

        Box(Modifier.fillMaxSize()) {
            if (expanded) {
                AdaptiveWorkspace(
                    store = store,
                    navController = navController,
                    confirmController = confirmController,
                    hardwareShortcut = hardwareShortcut,
                    onInspectorDismissCallback = { closeExpandedInspector = it },
                )
            } else {
                closeExpandedInspector = null
                S5NavGraph(
                    navController = navController,
                    store = store,
                    startDestination = Routes.Bootstrap,
                    widthSizeClass = widthSizeClass,
                    confirmController = confirmController,
                    hardwareShortcut = hardwareShortcut,
                    onHomeEscape = ::handleEscape,
                )
            }
            S5ActionProgressOverlay(
                progress = actionProgress,
                onDismiss = store::dismissActionProgress,
                modifier =
                    Modifier.statusBarsPadding()
                        .padding(horizontal = 12.dp, vertical = 56.dp),
            )
            S5GlobalErrorBanner(
                error = globalError,
                onDismiss = store::dismissGlobalError,
                modifier =
                    Modifier.statusBarsPadding()
                        .padding(horizontal = 12.dp, vertical = 8.dp),
            )
        }
        S5ConfirmDialogHost(confirmController)
    }
}

/**
 * Expanded workspace: persistent thread list, chat navigator, and an optional
 * independently navigated inspector. Tool routes opened from chat use the third
 * pane only when chat can retain its minimum width; narrower windows keep the
 * same routes but present them in the center navigator.
 */
@Composable
private fun AdaptiveWorkspace(
    store: AppStore,
    navController: NavHostController,
    confirmController: S5ConfirmDialogController,
    hardwareShortcut: S5HardwareShortcutEvent?,
    onInspectorDismissCallback: (((() -> Unit)?) -> Unit),
) {
    var inspectorRoute by rememberSaveable { mutableStateOf<String?>(null) }
    var inspectorOwner by rememberSaveable { mutableStateOf<String?>(null) }
    var preferredInspectorWidthDp by rememberSaveable { mutableStateOf<Float?>(null) }
    val currentEntry by navController.currentBackStackEntryAsState()
    val selectedThread =
        currentEntry?.arguments?.let { arguments ->
            val environmentId = arguments.getString("environmentId")
            val threadId = arguments.getString("threadId")
            if (environmentId != null && threadId != null) "$environmentId/$threadId" else null
        }

    fun closeInspector() {
        inspectorRoute = null
        inspectorOwner = null
    }
    LaunchedEffect(inspectorRoute) {
        onInspectorDismissCallback(if (inspectorRoute == null) null else ::closeInspector)
    }
    LaunchedEffect(selectedThread) {
        if (inspectorOwner != null && selectedThread != inspectorOwner) closeInspector()
    }

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val listWidth = WorkspacePaneSizing.ListWidthDp.dp
        val availableContentWidthDp = (maxWidth - listWidth).value.coerceAtLeast(0f)
        val dividerWidthDp = WORKSPACE_DIVIDER_TOUCH_WIDTH.value
        val inspectorSizingWidthDp = (availableContentWidthDp - dividerWidthDp).coerceAtLeast(0f)
        val supportsInspector =
            inspectorSizingWidthDp >=
                WorkspacePaneSizing.MinimumChatWidthDp + WorkspacePaneSizing.InspectorMinWidthDp
        val inspectorWidthDp =
            WorkspacePaneSizing.constrainInspectorWidth(
                preferredWidthDp =
                    preferredInspectorWidthDp
                        ?: WorkspacePaneSizing.defaultInspectorWidth(inspectorSizingWidthDp),
                availableWidthDp = inspectorSizingWidthDp,
            )
        val inspectorMaximumDp =
            WorkspacePaneSizing.constrainInspectorWidth(
                WorkspacePaneSizing.InspectorMaxWidthDp.toFloat(),
                inspectorSizingWidthDp,
            )

        LaunchedEffect(supportsInspector) {
            if (!supportsInspector && inspectorRoute != null) closeInspector()
        }

        Row(Modifier.fillMaxSize()) {
            Box(Modifier.width(listWidth).fillMaxHeight()) {
                HomeScreen(
                    store = store,
                    onOpenThread = { environmentId, threadId ->
                        closeInspector()
                        navController.navigate(Routes.thread(environmentId, threadId)) {
                            popUpTo(Routes.Home)
                        }
                    },
                    onNewTask = { navController.navigate(Routes.NewTask) },
                    onResumeDraft = { navController.navigate(Routes.NewTaskDraft) },
                    onSettings = { navController.navigate(Routes.Settings) },
                    onConnections = { navController.navigate(Routes.Connections) },
                    onArchive = { navController.navigate(Routes.Archive) },
                    confirmController = confirmController,
                    hardwareShortcut = hardwareShortcut,
                    onEscape = {
                        if (inspectorRoute != null) closeInspector()
                        else if (currentEntry?.destination?.route != Routes.WorkspaceEmpty) {
                            navController.popBackStack()
                        }
                    },
                )
            }
            Box(Modifier.width(1.dp).fillMaxHeight()) {
                androidx.compose.material3.VerticalDivider()
            }
            Box(Modifier.weight(1f).fillMaxHeight()) {
                S5NavGraph(
                    navController = navController,
                    store = store,
                    startDestination = Routes.WorkspaceEmpty,
                    widthSizeClass = S5WindowWidth.Expanded,
                    confirmController = confirmController,
                    onOpenInspector = { route ->
                        if (supportsInspector && selectedThread != null) {
                            inspectorRoute = route
                            inspectorOwner = selectedThread
                        } else {
                            navController.navigate(route)
                        }
                    },
                )
            }
            inspectorRoute?.takeIf { supportsInspector }?.let { route ->
                S5WorkspacePaneDivider(
                    currentWidthDp = inspectorWidthDp,
                    minimumWidthDp = WorkspacePaneSizing.InspectorMinWidthDp.toFloat(),
                    maximumWidthDp = inspectorMaximumDp,
                    onResizeBy = { delta ->
                        preferredInspectorWidthDp =
                            WorkspacePaneSizing.constrainInspectorWidth(
                                preferredWidthDp = inspectorWidthDp + delta,
                                availableWidthDp = inspectorSizingWidthDp,
                            )
                    },
                )
                Box(Modifier.width(inspectorWidthDp.dp).fillMaxHeight()) {
                    WorkspaceInspectorHost(
                        store = store,
                        startRoute = route,
                        confirmController = confirmController,
                        onDismiss = ::closeInspector,
                    )
                }
            }
        }
    }
}

/** Coarse width buckets; the only layout input features need. */
enum class S5WindowWidth {
    Compact,
    Medium,
    Expanded,
}
