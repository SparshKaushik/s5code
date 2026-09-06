package club.touchtech.s5code.kotlin.app

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Explore
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.navArgument
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.feature.archive.ArchiveScreen
import club.touchtech.s5code.kotlin.feature.connections.ConnectionDetailScreen
import club.touchtech.s5code.kotlin.feature.connections.ConnectionsScreen
import club.touchtech.s5code.kotlin.feature.files.FilePreviewScreen
import club.touchtech.s5code.kotlin.feature.files.FilesTreeScreen
import club.touchtech.s5code.kotlin.feature.files.ImagePreviewScreen
import club.touchtech.s5code.kotlin.feature.files.MarkdownPreviewScreen
import club.touchtech.s5code.kotlin.feature.files.WebPreviewScreen
import club.touchtech.s5code.kotlin.feature.git.GitBranchesScreen
import club.touchtech.s5code.kotlin.feature.git.GitCommitScreen
import club.touchtech.s5code.kotlin.feature.git.GitConfirmScreen
import club.touchtech.s5code.kotlin.feature.git.GitOverviewScreen
import club.touchtech.s5code.kotlin.feature.git.PullRequestsScreen
import club.touchtech.s5code.kotlin.feature.git.SourceControlScreen
import club.touchtech.s5code.kotlin.feature.home.HomeScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectDestinationScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectLocalPathScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectRepositoryScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectSourceScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskBranchScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskDraftScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskEnvironmentScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskProjectScreen
import club.touchtech.s5code.kotlin.feature.onboarding.AddEnvironmentScreen
import club.touchtech.s5code.kotlin.feature.onboarding.BootstrapScreen
import club.touchtech.s5code.kotlin.feature.onboarding.ConnectSetupScreen
import club.touchtech.s5code.kotlin.feature.onboarding.ConnectSignInScreen
import club.touchtech.s5code.kotlin.feature.onboarding.OnboardingScreen
import club.touchtech.s5code.kotlin.feature.onboarding.PairQrScreen
import club.touchtech.s5code.kotlin.feature.onboarding.PairUrlScreen
import club.touchtech.s5code.kotlin.feature.review.ReviewCommentScreen
import club.touchtech.s5code.kotlin.feature.review.ReviewScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsAccountScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsAppearanceScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsClientStorageScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsLiveUpdatesScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsNotificationsScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsProjectGroupingScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsScreen
import club.touchtech.s5code.kotlin.feature.terminal.TerminalScreen
import club.touchtech.s5code.kotlin.feature.thread.ThreadRewindScreen
import club.touchtech.s5code.kotlin.feature.thread.ThreadScreen
import club.touchtech.s5code.kotlin.feature.usage.UsageScreen

/**
 * The whole route table in one graph. Thread tool routes are flat rather than
 * nested so a notification or widget deep link can land directly on any of them.
 */
@Composable
fun S5NavGraph(
    navController: NavHostController,
    store: AppStore,
    startDestination: String,
    widthSizeClass: S5WindowWidth,
    confirmController: S5ConfirmDialogController,
    hardwareShortcut: S5HardwareShortcutEvent? = null,
    onOpenInspector: ((String) -> Unit)? = null,
    onHomeEscape: () -> Unit = {},
) {
    val environmentArg = navArgument("environmentId") { type = NavType.StringType }
    val threadArg = navArgument("threadId") { type = NavType.StringType }
    // File viewers are reachable with no path (a deep link that names only a
    // thread), so the argument defaults rather than failing to resolve.
    val pathArg =
        navArgument("path") {
            type = NavType.StringType
            defaultValue = ""
        }
    val fileArgs = listOf(environmentArg, threadArg, pathArg)
    // Absent means "not from onboarding", which is what a deep link into pairing
    // should behave like: it returns where it came from rather than replacing the
    // shell with home.
    val onboardingArgs =
        listOf(
            navArgument("onboarding") {
                type = NavType.BoolType
                defaultValue = false
            }
        )
    val transitions = s5NavTransitions()

    NavHost(
        navController = navController,
        startDestination = startDestination,
        enterTransition = transitions.enter,
        exitTransition = transitions.exit,
        popEnterTransition = transitions.popEnter,
        popExitTransition = transitions.popExit,
    ) {
        composable(Routes.WorkspaceEmpty) {
            S5EmptyState(
                icon = Icons.Rounded.Explore,
                title = "Pick a thread",
                detail = "Select a thread on the left, or start a new task.",
                actionLabel = "New task",
                onAction = { navController.navigate(Routes.NewTask) },
            )
        }

        composable(Routes.Bootstrap) {
            BootstrapScreen(
                store = store,
                // Both routes clear the splash so back from the first real screen
                // exits the app rather than returning to a dead destination.
                onPaired = {
                    navController.navigate(Routes.Home) {
                        popUpTo(Routes.Bootstrap) { inclusive = true }
                    }
                },
                onUnpaired = {
                    navController.navigate(Routes.Onboarding) {
                        popUpTo(Routes.Bootstrap) { inclusive = true }
                    }
                },
            )
        }

        composable(Routes.Onboarding) {
            OnboardingScreen(
                onPairUrl = { navController.navigate(Routes.pairUrl(onboarding = true)) },
                onPairQr = { navController.navigate(Routes.pairQr(onboarding = true)) },
                onConnect = { navController.navigate(Routes.connectSignIn(onboarding = true)) },
            )
        }
        composable(Routes.PairUrl, arguments = onboardingArgs) { entry ->
            val onboarding = entry.arguments?.getBoolean("onboarding") == true
            PairUrlScreen(
                store = store,
                onBack = navController::popBackStack,
                onPaired = { navController.finishPairing(onboarding) },
                onScanQr = { navController.navigate(Routes.pairQr(onboarding)) },
            )
        }
        composable(Routes.PairQr, arguments = onboardingArgs) { entry ->
            val onboarding = entry.arguments?.getBoolean("onboarding") == true
            PairQrScreen(
                store = store,
                onBack = navController::popBackStack,
                onManual = { navController.navigate(Routes.pairUrl(onboarding)) },
                onPaired = { navController.finishPairing(onboarding) },
            )
        }
        composable(Routes.ConnectSignIn, arguments = onboardingArgs) { entry ->
            val onboarding = entry.arguments?.getBoolean("onboarding") == true
            ConnectSignInScreen(
                store = store,
                onBack = navController::popBackStack,
                onContinue = { navController.navigate(Routes.connectSetup(onboarding)) },
            )
        }
        composable(Routes.ConnectSetup, arguments = onboardingArgs) { entry ->
            val onboarding = entry.arguments?.getBoolean("onboarding") == true
            ConnectSetupScreen(
                store = store,
                onBack = navController::popBackStack,
                onDone = { navController.finishPairing(onboarding) },
            )
        }

        composable(Routes.Home) {
            HomeScreen(
                store = store,
                onOpenThread = { environmentId, threadId ->
                    navController.navigate(Routes.thread(environmentId, threadId))
                },
                onNewTask = { navController.navigate(Routes.NewTask) },
                // Straight to the draft: it already has a project, and sending the
                // user back through the picker is what made a saved draft feel gone.
                onResumeDraft = { navController.navigate(Routes.NewTaskDraft) },
                onSettings = { navController.navigate(Routes.Settings) },
                onConnections = { navController.navigate(Routes.Connections) },
                onArchive = { navController.navigate(Routes.Archive) },
                confirmController = confirmController,
                hardwareShortcut = hardwareShortcut,
                onEscape = onHomeEscape,
            )
        }

        composable(Routes.Connections) {
            ConnectionsScreen(
                store = store,
                onBack = navController::popBackStack,
                onAdd = { navController.navigate(Routes.ConnectionsNew) },
                onOpen = { environmentId -> navController.navigate(Routes.connectionDetail(environmentId)) },
            )
        }
        // Adding an environment from an already-paired app is the same choice as
        // first-run onboarding (direct pairing or S5 Connect), so it reuses that
        // screen on its own route instead of dropping the user straight into the
        // URL form with no way to pick Connect.
        composable(Routes.ConnectionsNew) {
            AddEnvironmentScreen(
                onBack = navController::popBackStack,
                onPairUrl = { navController.navigate(Routes.pairUrl(onboarding = false)) },
                onPairQr = { navController.navigate(Routes.pairQr(onboarding = false)) },
                onConnect = { navController.navigate(Routes.connectSignIn(onboarding = false)) },
            )
        }
        composable(Routes.ConnectionDetail, arguments = listOf(environmentArg)) { entry ->
            ConnectionDetailScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                onBack = navController::popBackStack,
                onRemoved = navController::popBackStack,
                confirmController = confirmController,
            )
        }

        composable(Routes.NewTask) {
            NewTaskProjectScreen(
                store = store,
                onBack = navController::popBackStack,
                onProjectChosen = { navController.navigate(Routes.NewTaskDraft) },
                onAddProject = { navController.navigate(Routes.AddProjectSource) },
            )
        }
        composable(Routes.NewTaskDraft) {
            NewTaskDraftScreen(
                store = store,
                onBack = navController::popBackStack,
                // The project name in the hero is a way back to the picker, and
                // popping rather than pushing keeps the flow two screens deep no
                // matter how many times you change your mind.
                onProject = navController::popBackStack,
                onEnvironment = { navController.navigate(Routes.NewTaskEnvironment) },
                onBranch = { navController.navigate(Routes.NewTaskBranch) },
                onCreated = { environmentId, threadId ->
                    navController.navigate(Routes.thread(environmentId, threadId)) {
                        popUpTo(Routes.Home)
                    }
                },
            )
        }
        composable(Routes.NewTaskEnvironment) {
            NewTaskEnvironmentScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.NewTaskBranch) {
            NewTaskBranchScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.AddProjectSource) {
            AddProjectSourceScreen(
                onBack = navController::popBackStack,
                onRepository = { navController.navigate(Routes.AddProjectRepository) },
                onLocalPath = { navController.navigate(Routes.AddProjectLocal) },
            )
        }
        composable(Routes.AddProjectRepository) {
            AddProjectRepositoryScreen(
                store = store,
                onBack = navController::popBackStack,
                onSelected = { navController.navigate(Routes.AddProjectDestination) },
            )
        }
        composable(Routes.AddProjectDestination) {
            AddProjectDestinationScreen(
                store = store,
                onBack = navController::popBackStack,
                onCreated = { navController.popBackStack(Routes.NewTask, inclusive = false) },
            )
        }
        composable(Routes.AddProjectLocal) {
            AddProjectLocalPathScreen(
                store = store,
                onBack = navController::popBackStack,
                onCreated = { navController.popBackStack(Routes.NewTask, inclusive = false) },
            )
        }

        composable(Routes.Thread, arguments = listOf(environmentArg, threadArg)) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            // Recorded for launcher shortcuts. Keyed on the title too, because the
            // shell that carries titles arrives after navigation and the first
            // record of a thread often has none.
            val title =
                store.workspace.threads.collectAsStateWithLifecycle().value
                    .firstOrNull { it.environmentId.value == environmentId && it.id.value == threadId }
                    ?.title
                    .orEmpty()
            LaunchedEffect(environmentId, threadId, title) {
                store.recordRecentThread(environmentId, threadId, title)
            }
            ThreadScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                onBack = navController::popBackStack,
                onOpen = { suffix ->
                    val route = Routes.threadChild(environmentId, threadId, suffix)
                    if (isWorkspaceInspectorSuffix(suffix)) {
                        onOpenInspector?.invoke(route) ?: navController.navigate(route)
                    } else {
                        navController.navigate(route)
                    }
                },
            )
        }
        composable(Routes.ThreadRewind, arguments = listOf(environmentArg, threadArg)) { entry ->
            ThreadRewindScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
                confirmController = confirmController,
            )
        }
        composable(Routes.ThreadFiles, arguments = listOf(environmentArg, threadArg)) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            FilesTreeScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                onBack = navController::popBackStack,
                onOpenFile = { suffix ->
                    navController.navigate(Routes.threadChild(environmentId, threadId, suffix))
                },
            )
        }
        composable(Routes.ThreadFile, arguments = fileArgs) { entry ->
            FilePreviewScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                path = entry.arguments?.getString("path").orEmpty(),
                onBack = navController::popBackStack,
            )
        }
        composable(Routes.ThreadFileMarkdown, arguments = fileArgs) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            MarkdownPreviewScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                path = entry.arguments?.getString("path").orEmpty(),
                onBack = navController::popBackStack,
                onOpenFile = { path ->
                    navController.navigate(Routes.threadFilePreview(environmentId, threadId, path))
                },
            )
        }
        composable(Routes.ThreadFileImage, arguments = fileArgs) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            val path = entry.arguments?.getString("path").orEmpty()
            ImagePreviewScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                path = path,
                onBack = navController::popBackStack,
                onOpenSource = {
                    navController.navigate(Routes.threadFileSource(environmentId, threadId, path))
                },
            )
        }
        composable(Routes.ThreadFileWeb, arguments = fileArgs) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            val path = entry.arguments?.getString("path").orEmpty()
            WebPreviewScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                path = path,
                onBack = navController::popBackStack,
                onOpenSource = {
                    navController.navigate(Routes.threadFileSource(environmentId, threadId, path))
                },
            )
        }
        composable(Routes.ThreadReview, arguments = listOf(environmentArg, threadArg)) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            ReviewScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                onBack = navController::popBackStack,
                onComment = { target ->
                    navController.navigate(
                        Routes.threadReviewComment(
                            environmentId = environmentId,
                            threadId = threadId,
                            filePath = target.filePath,
                            startIndex = target.normalizedStart,
                            endIndex = target.normalizedEnd,
                        )
                    )
                },
            )
        }
        composable(
            Routes.ThreadReviewComment,
            arguments =
                listOf(
                    environmentArg,
                    threadArg,
                    navArgument("filePath") { defaultValue = "" },
                    navArgument("startIndex") {
                        type = NavType.IntType
                        defaultValue = 0
                    },
                    navArgument("endIndex") {
                        type = NavType.IntType
                        defaultValue = 0
                    },
                ),
        ) { entry ->
            ReviewCommentScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                filePath = entry.arguments?.getString("filePath").orEmpty(),
                startIndex = entry.arguments?.getInt("startIndex") ?: 0,
                endIndex = entry.arguments?.getInt("endIndex") ?: 0,
                onBack = navController::popBackStack,
            )
        }
        composable(Routes.ThreadTerminal, arguments = listOf(environmentArg, threadArg)) { entry ->
            TerminalScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
                confirmController = confirmController,
            )
        }
        composable(Routes.Git, arguments = listOf(environmentArg, threadArg)) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            GitOverviewScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                onBack = navController::popBackStack,
                onOpen = { suffix ->
                    navController.navigate(Routes.threadChild(environmentId, threadId, suffix))
                },
            )
        }
        composable(Routes.GitCommit, arguments = listOf(environmentArg, threadArg)) { entry ->
            GitCommitScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
            )
        }
        composable(Routes.GitBranches, arguments = listOf(environmentArg, threadArg)) { entry ->
            GitBranchesScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
            )
        }
        composable(Routes.GitConfirm, arguments = listOf(environmentArg, threadArg)) { entry ->
            GitConfirmScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
            )
        }
        composable(Routes.SourceControl, arguments = listOf(environmentArg, threadArg)) { entry ->
            val environmentId = entry.arguments?.getString("environmentId").orEmpty()
            val threadId = entry.arguments?.getString("threadId").orEmpty()
            SourceControlScreen(
                store = store,
                environmentId = environmentId,
                threadId = threadId,
                onBack = navController::popBackStack,
                onOpen = { suffix ->
                    navController.navigate(Routes.threadChild(environmentId, threadId, suffix))
                },
            )
        }
        composable(Routes.PullRequests, arguments = listOf(environmentArg, threadArg)) { entry ->
            PullRequestsScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                onBack = navController::popBackStack,
            )
        }

        composable(Routes.Settings) {
            SettingsScreen(
                store = store,
                onBack = navController::popBackStack,
                onOpen = { route -> navController.navigate(route) },
            )
        }
        composable(Routes.SettingsAccount) {
            SettingsAccountScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsEnvironments) {
            // Same screen and the same add flow as Routes.Connections. Two routes
            // exist because both deep links are real (Settings pushes one, the home
            // toolbar the other), but they must not behave differently: this one
            // used to send "Add" straight to the URL form, which silently removed
            // S5 Connect as an option for anyone who arrived through Settings.
            ConnectionsScreen(
                store = store,
                onBack = navController::popBackStack,
                onAdd = { navController.navigate(Routes.ConnectionsNew) },
                onOpen = { environmentId -> navController.navigate(Routes.connectionDetail(environmentId)) },
            )
        }
        composable(Routes.SettingsAppearance) {
            SettingsAppearanceScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsProjectGrouping) {
            SettingsProjectGroupingScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsNotifications) {
            SettingsNotificationsScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsLiveUpdates) {
            SettingsLiveUpdatesScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsClientStorage) {
            SettingsClientStorageScreen(
                store = store,
                onBack = navController::popBackStack,
                confirmController = confirmController,
            )
        }
        composable(Routes.Usage) { UsageScreen(store = store, onBack = navController::popBackStack) }
        composable(Routes.Archive) {
            ArchiveScreen(
                store = store,
                onBack = navController::popBackStack,
                onOpenThread = { environmentId, threadId ->
                    navController.navigate(Routes.thread(environmentId, threadId))
                },
                confirmController = confirmController,
            )
        }
        composable(Routes.NotFound) {
            S5EmptyState(
                icon = Icons.Rounded.Explore,
                title = "Nothing here",
                detail = "That link points at a screen this version doesn't have.",
                actionLabel = "Go home",
                onAction = {
                    navController.navigate(Routes.Home) { popUpTo(Routes.Home) { inclusive = true } }
                },
            )
        }
    }
}

/**
 * Leaves the pairing flow. A first run ends at home with onboarding dropped;
 * pairing a second environment unwinds to the connections list that now contains
 * it.
 */
private fun NavHostController.finishPairing(fromOnboarding: Boolean) {
    if (fromOnboarding) {
        navigate(Routes.Home) { popUpTo(Routes.Onboarding) { inclusive = true } }
    } else {
        popBackStack(Routes.Connections, inclusive = false)
    }
}
