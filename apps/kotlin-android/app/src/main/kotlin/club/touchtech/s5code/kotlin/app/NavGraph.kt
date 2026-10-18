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
import club.touchtech.s5code.kotlin.data.PullRequestReviewPosition
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestDiffScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestDiscussionScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestFilesScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestInlineCommentScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestGuideStopScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestReviewScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestReviewersScreen
import club.touchtech.s5code.kotlin.data.GuideDiffFocus
import club.touchtech.s5code.kotlin.feature.pullrequests.ThreadPullRequestsScreen
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.feature.archive.ArchiveScreen
import club.touchtech.s5code.kotlin.feature.connections.ConnectionDetailScreen
import club.touchtech.s5code.kotlin.feature.connections.ConnectionsScreen
import club.touchtech.s5code.kotlin.feature.devices.DevicePreviewScreen
import club.touchtech.s5code.kotlin.feature.files.AttachmentFileScreen
import club.touchtech.s5code.kotlin.feature.files.FilePreviewScreen
import club.touchtech.s5code.kotlin.feature.files.FilesTreeScreen
import club.touchtech.s5code.kotlin.feature.files.ImagePreviewScreen
import club.touchtech.s5code.kotlin.feature.files.MarkdownPreviewScreen
import club.touchtech.s5code.kotlin.feature.files.WebPreviewScreen
import club.touchtech.s5code.kotlin.feature.git.GitBranchesScreen
import club.touchtech.s5code.kotlin.feature.git.GitCommitScreen
import club.touchtech.s5code.kotlin.feature.git.GitConfirmScreen
import club.touchtech.s5code.kotlin.feature.git.GitOverviewScreen
import club.touchtech.s5code.kotlin.feature.pullrequests.PullRequestDetailScreen
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.feature.git.SourceControlScreen
import club.touchtech.s5code.kotlin.feature.home.HomeScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectDestinationScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectLocalFolderScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectRepositoryScreen
import club.touchtech.s5code.kotlin.feature.newtask.AddProjectSourceScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskBranchScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskDraftScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskEnvironmentScreen
import club.touchtech.s5code.kotlin.feature.newtask.NewTaskProjectScreen
import club.touchtech.s5code.kotlin.feature.onboarding.BootstrapScreen
import club.touchtech.s5code.kotlin.feature.onboarding.ConnectSignInScreen
import club.touchtech.s5code.kotlin.feature.onboarding.OnboardingScreen
import club.touchtech.s5code.kotlin.feature.onboarding.PairUrlScreen
import club.touchtech.s5code.kotlin.feature.review.ReviewCommentScreen
import club.touchtech.s5code.kotlin.feature.review.ReviewScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsAccountScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsAgentBehaviorScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsAppearanceScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsClientStorageScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsDiagnosticsScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsFollowUpsScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsMaintenanceScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsNewThreadsScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsNotificationsScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsOverviewScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsProjectGroupingScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsSourceControlScreen
import club.touchtech.s5code.kotlin.feature.settings.SettingsThreadsScreen
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
    // Deep links may carry the parsed host/code to prefill the form and a
    // scan flag that opens the camera inline, as RN's `mode=scan_qr` does.
    val pairUrlArgs =
        onboardingArgs +
            listOf(
                navArgument("host") {
                    type = NavType.StringType
                    defaultValue = ""
                },
                navArgument("code") {
                    type = NavType.StringType
                    defaultValue = ""
                },
                navArgument("scan") {
                    type = NavType.BoolType
                    defaultValue = false
                },
            )
    // Add-project steps carry the chosen environment (and the repository the
    // lookup resolved) as params, as RN's routes do.
    val addProjectArgs =
        listOf(
            navArgument("environmentId") {
                type = NavType.StringType
                defaultValue = ""
            },
            navArgument("source") {
                type = NavType.StringType
                defaultValue = ""
            },
        )
    val addProjectDestinationArgs =
        addProjectArgs +
            listOf(
                navArgument("remoteUrl") {
                    type = NavType.StringType
                    nullable = true
                    defaultValue = null
                },
                navArgument("repositoryTitle") {
                    type = NavType.StringType
                    nullable = true
                    defaultValue = null
                },
                navArgument("repositoryName") {
                    type = NavType.StringType
                    defaultValue = ""
                },
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
                onPairQr = {
                    navController.navigate(Routes.pairUrl(onboarding = true, scan = true))
                },
                onConnect = { navController.navigate(Routes.connectSignIn(onboarding = true)) },
            )
        }
        composable(Routes.PairUrl, arguments = pairUrlArgs) { entry ->
            val onboarding = entry.arguments?.getBoolean("onboarding") == true
            PairUrlScreen(
                store = store,
                onBack = navController::popBackStack,
                onPaired = { navController.finishPairing(onboarding) },
                initialHost = entry.arguments?.getString("host").orEmpty(),
                initialCode = entry.arguments?.getString("code").orEmpty(),
                startScanning = entry.arguments?.getBoolean("scan") == true,
            )
        }
        composable(Routes.ConnectSignIn, arguments = onboardingArgs) { entry ->
            val onboarding = entry.arguments?.getBoolean("onboarding") == true
            ConnectSignInScreen(
                store = store,
                onBack = navController::popBackStack,
                // RN has no managed-machines step: signing in lands on the
                // environments list, which discovers and toggles them inline.
                onContinue = {
                    if (onboarding) {
                        navController.finishPairing(fromOnboarding = true)
                    } else {
                        navController.popBackStack()
                    }
                },
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
                onOpenPullRequest = { environmentId, reference ->
                    navController.navigate(Routes.pullRequest(environmentId, reference))
                },
                onRequestPullRequestReview = { environmentId, reference ->
                    navController.navigate(Routes.pullRequestReviewers(environmentId, reference))
                },
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
        // RN's add flow is only the pairing form — S5 Connect machines are
        // discovered and toggled from the environments list itself, never
        // added through a separate chooser.
        composable(Routes.ConnectionsNew) {
            PairUrlScreen(
                store = store,
                onBack = navController::popBackStack,
                onPaired = { navController.finishPairing(fromOnboarding = false) },
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
                onAddEnvironment = { navController.navigate(Routes.ConnectionsNew) },
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
        // RN's flow keeps the environment on each step's params, and a finished
        // add lands on the new-task draft bound to the new project rather than
        // back on the picker.
        composable(Routes.AddProjectSource) {
            AddProjectSourceScreen(
                store = store,
                onBack = navController::popBackStack,
                onProjectCreated = { navController.navigate(Routes.NewTaskDraft) { popUpTo(Routes.AddProjectSource) { inclusive = true } } },
                onRepository = { environmentId, source ->
                    navController.navigate(Routes.addProjectRepository(environmentId, source))
                },
                onLocalPath = { environmentId ->
                    navController.navigate(Routes.addProjectLocal(environmentId))
                },
                onAddEnvironment = {
                    navController.navigate(Routes.ConnectionsNew) {
                        popUpTo(Routes.AddProjectSource) { inclusive = true }
                    }
                },
            )
        }
        composable(Routes.AddProjectRepository, arguments = addProjectArgs) { entry ->
            AddProjectRepositoryScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                source = entry.arguments?.getString("source").orEmpty().ifBlank { "url" },
                onBack = navController::popBackStack,
                onDestination = { envId, source, remoteUrl, repositoryTitle, repositoryName ->
                    navController.navigate(
                        Routes.addProjectDestination(
                            envId, source, remoteUrl, repositoryTitle, repositoryName,
                        )
                    )
                },
                onAddEnvironment = {
                    navController.navigate(Routes.ConnectionsNew) {
                        popUpTo(Routes.AddProjectSource) { inclusive = true }
                    }
                },
            )
        }
        composable(Routes.AddProjectDestination, arguments = addProjectDestinationArgs) { entry ->
            AddProjectDestinationScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                remoteUrl = entry.arguments?.getString("remoteUrl"),
                repositoryTitle = entry.arguments?.getString("repositoryTitle"),
                repositoryName = entry.arguments?.getString("repositoryName").orEmpty(),
                onBack = navController::popBackStack,
                onCreated = { environmentId, projectId, cloning ->
                    store.updateDraft {
                        it.copy(
                            environmentId = EnvironmentId(environmentId),
                            projectKey = projectId,
                            cloning = cloning,
                        )
                    }
                    navController.navigate(Routes.NewTaskDraft) {
                        popUpTo(Routes.NewTask) { inclusive = true }
                    }
                },
                onAddEnvironment = {
                    navController.navigate(Routes.ConnectionsNew) {
                        popUpTo(Routes.AddProjectSource) { inclusive = true }
                    }
                },
            )
        }
        composable(Routes.AddProjectLocal, arguments = addProjectArgs) { entry ->
            AddProjectLocalFolderScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                onBack = navController::popBackStack,
                onCreated = { environmentId, projectId ->
                    store.updateDraft {
                        it.copy(
                            environmentId = EnvironmentId(environmentId),
                            projectKey = projectId,
                            cloning = false,
                        )
                    }
                    navController.navigate(Routes.NewTaskDraft) {
                        popUpTo(Routes.NewTask) { inclusive = true }
                    }
                },
                onAddEnvironment = {
                    navController.navigate(Routes.ConnectionsNew) {
                        popUpTo(Routes.AddProjectSource) { inclusive = true }
                    }
                },
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
                onOpenThread = { envId, nextThreadId ->
                    navController.navigate(Routes.thread(envId, nextThreadId))
                },
            )
        }
        composable(Routes.ThreadDevices, arguments = listOf(environmentArg, threadArg)) { entry ->
            DevicePreviewScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
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
        composable(Routes.ThreadBrowser, arguments = listOf(environmentArg, threadArg)) { entry ->
            club.touchtech.s5code.kotlin.feature.browser.BrowserScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                onBack = navController::popBackStack,
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
        composable(
            Routes.ThreadAttachment,
            arguments =
                listOf(
                    environmentArg,
                    threadArg,
                    navArgument("attachmentId") { type = NavType.StringType },
                    // A deep link may carry only the id; the params describe the
                    // file, so they default rather than blocking the route.
                    navArgument("name") {
                        type = NavType.StringType
                        defaultValue = ""
                    },
                    navArgument("mimeType") {
                        type = NavType.StringType
                        defaultValue = ""
                    },
                    navArgument("sizeBytes") {
                        type = NavType.LongType
                        defaultValue = 0L
                    },
                ),
        ) { entry ->
            AttachmentFileScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                attachmentId = entry.arguments?.getString("attachmentId").orEmpty(),
                name = entry.arguments?.getString("name").orEmpty(),
                mimeType = entry.arguments?.getString("mimeType").orEmpty(),
                sizeBytes = entry.arguments?.getLong("sizeBytes") ?: 0L,
                onBack = navController::popBackStack,
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
        composable(
            Routes.ThreadTerminal,
            arguments =
                listOf(
                    environmentArg,
                    threadArg,
                    // Nullable so `threads/e/t/terminal` still resolves to the
                    // default shell; only session switches carry an id.
                    navArgument("terminalId") {
                        type = NavType.StringType
                        nullable = true
                    },
                ),
        ) { entry ->
            val terminalEnvironmentId = entry.arguments?.getString("environmentId").orEmpty()
            val terminalThreadId = entry.arguments?.getString("threadId").orEmpty()
            TerminalScreen(
                store = store,
                environmentId = terminalEnvironmentId,
                threadId = terminalThreadId,
                terminalId = entry.arguments?.getString("terminalId"),
                onBack = navController::popBackStack,
                // RN `StackActions.replace`: the dead session leaves the stack.
                onSwitchTerminal = { nextTerminalId ->
                    navController.navigate(
                        Routes.threadTerminal(terminalEnvironmentId, terminalThreadId, nextTerminalId)
                    ) {
                        popUpTo(Routes.ThreadTerminal) { inclusive = true }
                    }
                },
                onExitToThread = {
                    if (!navController.popBackStack()) {
                        navController.navigate(Routes.thread(terminalEnvironmentId, terminalThreadId)) {
                            popUpTo(Routes.ThreadTerminal) { inclusive = true }
                        }
                    }
                },
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
                onOpenPullRequest = { targetEnvironmentId, reference ->
                    navController.navigate(Routes.pullRequest(targetEnvironmentId, reference))
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
        val pullRequestArgs =
            listOf(
                environmentArg,
                navArgument("projectId") { type = NavType.StringType },
                navArgument("repository") { type = NavType.StringType },
                navArgument("number") { type = NavType.IntType },
                optionalStringArg("host"),
            )
        fun pullRequestReference(arguments: android.os.Bundle?) =
            PullRequestReference(
                projectId = arguments?.getString("projectId").orEmpty(),
                repository = arguments?.getString("repository").orEmpty(),
                number = arguments?.getInt("number") ?: 0,
                host = arguments?.getString("host")?.takeIf(String::isNotEmpty),
            )
        val openPullRequestThread: (String, String) -> Unit = { environmentId, threadId ->
            navController.navigate(Routes.thread(environmentId, threadId))
        }
        val openPullRequestNewTask: () -> Unit = { navController.navigate(Routes.NewTaskDraft) }
        composable(Routes.PullRequest, arguments = pullRequestArgs) { entry ->
            PullRequestDetailScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(entry.arguments),
                confirmController = confirmController,
                onBack = navController::popBackStack,
                onOpenPullRequest = { environmentId, reference ->
                    navController.navigate(Routes.pullRequest(environmentId, reference))
                },
                onOpenRoute = { route -> navController.navigate(route) },
                onOpenThread = openPullRequestThread,
                onOpenNewTask = openPullRequestNewTask,
            )
        }
        composable(Routes.PullRequestFiles, arguments = pullRequestArgs + optionalStringArg("commit")) { entry ->
            PullRequestFilesScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(entry.arguments),
                commit = entry.arguments?.getString("commit")?.takeIf(String::isNotEmpty),
                onBack = navController::popBackStack,
                onOpenRoute = { route -> navController.navigate(route) },
            )
        }
        composable(
            Routes.PullRequestDiff,
            arguments =
                pullRequestArgs + optionalStringArg("path") + optionalStringArg("commit") +
                    optionalStringArg("replacingDraftId") + optionalStringArg("focus"),
        ) { entry ->
            val arguments = entry.arguments
            PullRequestDiffScreen(
                store = store,
                environmentId = arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(arguments),
                path = arguments?.getString("path")?.takeIf(String::isNotEmpty),
                commit = arguments?.getString("commit")?.takeIf(String::isNotEmpty),
                replacingDraftId = arguments?.getString("replacingDraftId")?.takeIf(String::isNotEmpty),
                focus = GuideDiffFocus.parse(arguments?.getString("focus")),
                confirmController = confirmController,
                onBack = navController::popBackStack,
                onOpenRoute = { route -> navController.navigate(route) },
                onOpenThread = openPullRequestThread,
                onOpenNewTask = openPullRequestNewTask,
            )
        }
        composable(
            Routes.PullRequestDiscussion,
            arguments = pullRequestArgs + navArgument("discussionId") { type = NavType.StringType },
        ) { entry ->
            PullRequestDiscussionScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(entry.arguments),
                discussionId = entry.arguments?.getString("discussionId").orEmpty(),
                onBack = navController::popBackStack,
                onOpenRoute = { route -> navController.navigate(route) },
            )
        }
        composable(
            Routes.PullRequestInlineComment,
            arguments =
                pullRequestArgs +
                    listOf("draftId", "path", "oldPath", "position", "headSha", "code").map(::optionalStringArg),
        ) { entry ->
            val arguments = entry.arguments
            val position =
                arguments?.getString("position")?.let { raw ->
                    runCatching {
                        kotlinx.serialization.json.Json.decodeFromString(PullRequestReviewPosition.serializer(), raw)
                    }.getOrNull()
                }
            val headSha = arguments?.getString("headSha")
            if (position == null || headSha.isNullOrEmpty()) {
                LaunchedEffect(Unit) { navController.popBackStack() }
            } else {
                PullRequestInlineCommentScreen(
                    store = store,
                    environmentId = arguments.getString("environmentId").orEmpty(),
                    reference = pullRequestReference(arguments),
                    draftId = arguments.getString("draftId")?.takeIf(String::isNotEmpty),
                    path = arguments.getString("path").orEmpty(),
                    oldPath = arguments.getString("oldPath")?.takeIf(String::isNotEmpty),
                    position = position,
                    headSha = headSha,
                    code = arguments.getString("code").orEmpty(),
                    confirmController = confirmController,
                    onBack = navController::popBackStack,
                )
            }
        }
        composable(Routes.PullRequestReview, arguments = pullRequestArgs + optionalStringArg("mode")) { entry ->
            PullRequestReviewScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(entry.arguments),
                mode = entry.arguments?.getString("mode") ?: "review",
                onBack = navController::popBackStack,
                onReplaceRoute = { route ->
                    navController.popBackStack()
                    navController.navigate(route)
                },
            )
        }
        composable(Routes.PullRequestReviewers, arguments = pullRequestArgs) { entry ->
            PullRequestReviewersScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(entry.arguments),
                onBack = navController::popBackStack,
            )
        }
        composable(
            Routes.PullRequestGuideStop,
            arguments =
                pullRequestArgs + navArgument("position") { type = NavType.IntType } +
                    optionalStringArg("guideHeadSha") + optionalStringArg("guideGeneratedAt"),
        ) { entry ->
            val arguments = entry.arguments
            PullRequestGuideStopScreen(
                store = store,
                environmentId = arguments?.getString("environmentId").orEmpty(),
                reference = pullRequestReference(arguments),
                position = arguments?.getInt("position") ?: 0,
                guideHeadSha = arguments?.getString("guideHeadSha").orEmpty(),
                guideGeneratedAt = arguments?.getString("guideGeneratedAt").orEmpty(),
                onBack = navController::popBackStack,
                onOpenRoute = { route -> navController.navigate(route) },
                onFinishTour = { navController.popBackStack(Routes.PullRequest, inclusive = false) },
            )
        }
        composable(Routes.PullRequests, arguments = listOf(environmentArg, threadArg)) { entry ->
            ThreadPullRequestsScreen(
                store = store,
                environmentId = entry.arguments?.getString("environmentId").orEmpty(),
                threadId = entry.arguments?.getString("threadId").orEmpty(),
                confirmController = confirmController,
                onBack = navController::popBackStack,
                onOpenPullRequest = { environmentId, reference ->
                    navController.navigate(Routes.pullRequest(environmentId, reference))
                },
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
        composable("settings/scheduled-tasks") {
            club.touchtech.s5code.kotlin.feature.settings.ScheduledTasksScreen(store, navController::popBackStack)
        }
        composable("settings/provider-accounts") {
            club.touchtech.s5code.kotlin.feature.settings.ProviderAccountsScreen(store, navController::popBackStack)
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
        // RN links both project-grouping and organization at the same screen.
        composable(Routes.SettingsOrganization) {
            SettingsProjectGroupingScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsFollowUps) {
            SettingsFollowUpsScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsThreads) {
            SettingsThreadsScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsNotifications) {
            SettingsNotificationsScreen(
                store = store,
                onBack = navController::popBackStack,
                onOpen = { route -> navController.navigate(route) },
            )
        }
        composable(Routes.SettingsOverview) {
            SettingsOverviewScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsNewThreads) {
            SettingsNewThreadsScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsSourceControl) {
            SettingsSourceControlScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsAgentBehavior) {
            SettingsAgentBehaviorScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsMaintenance) {
            SettingsMaintenanceScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsDiagnostics) {
            SettingsDiagnosticsScreen(store = store, onBack = navController::popBackStack)
        }
        // RN's settings/usage and settings/archive deep links resolve to the
        // same screens the top-level routes do.
        composable(Routes.SettingsUsage) {
            UsageScreen(store = store, onBack = navController::popBackStack)
        }
        composable(Routes.SettingsArchive) {
            ArchiveScreen(
                store = store,
                onBack = navController::popBackStack,
                onOpenThread = { environmentId, threadId ->
                    navController.navigate(Routes.thread(environmentId, threadId))
                },
                confirmController = confirmController,
            )
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
    } else if (!popBackStack()) {
        // A deep-linked form has nothing beneath it; home is the fallback,
        // matching RN's `canGoBack ? goBack : replace(Home)`.
        navigate(Routes.Home)
    }
}

private fun optionalStringArg(name: String) =
    navArgument(name) {
        type = NavType.StringType
        nullable = true
        defaultValue = null
    }
