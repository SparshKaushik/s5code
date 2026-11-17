package club.touchtech.s5code.kotlin.app

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import club.touchtech.s5code.kotlin.data.LinkedDraftAction
import club.touchtech.s5code.kotlin.data.LinkedDraftRequest
import club.touchtech.s5code.kotlin.data.LinkedPrompt
import club.touchtech.s5code.kotlin.data.NewThreadLinkPlan
import club.touchtech.s5code.kotlin.data.PromptLinkModelSelection
import club.touchtech.s5code.kotlin.data.PromptLinkRepository
import club.touchtech.s5code.kotlin.data.PromptLinkResult
import club.touchtech.s5code.kotlin.data.latestThreadEnvironmentIds
import club.touchtech.s5code.kotlin.data.linkedNewTaskDraftAction
import club.touchtech.s5code.kotlin.data.linkedThreadDraftAction
import club.touchtech.s5code.kotlin.data.parseNewThreadLink
import club.touchtech.s5code.kotlin.data.parseThreadLinkPrompt
import club.touchtech.s5code.kotlin.data.planNewThreadLink
import club.touchtech.s5code.kotlin.data.promptLinkNotice
import club.touchtech.s5code.kotlin.data.promptLinkScratchEnvironmentId
import club.touchtech.s5code.kotlin.data.promptLinkWaitingForShells
import club.touchtech.s5code.kotlin.data.resolvePromptLinkModelSelection
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogRequest
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.ThreadSettings
import club.touchtech.s5code.kotlin.model.WorkspaceMode
import kotlin.coroutines.resume
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine

/**
 * Fills drafts from prompt links: `usePromptLinkResolver` and
 * `useThreadPromptLink` in the RN client. The link's route opens first through
 * the ordinary deep-link path; the draft lands on top once the restored drafts
 * and, for new-task links, the environments' shells are in.
 */
@Composable
fun PromptLinkEffect(
    store: AppStore,
    navController: NavHostController,
    confirmController: S5ConfirmDialogController,
    restored: Boolean,
    paired: Boolean,
) {
    val pending by store.pendingPromptLink.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val resolver =
        remember(store, navController, confirmController) {
            PromptLinkResolver(store, navController, confirmController)
        }
    LaunchedEffect(pending, restored, paired) {
        val link = pending ?: return@LaunchedEffect
        if (!restored) return@LaunchedEffect
        if (!paired) {
            store.consumePendingPromptLink()
            return@LaunchedEffect
        }
        store.pendingLink.first { it == null }
        store.clientStateLoaded.first { it }
        if (link is DeepLink.NewTask) {
            store.workspace.environments.first { !promptLinkWaitingForShells(it) }
        }
        // Consuming changes this effect's key, so the resolution itself runs in
        // a scope that outlives it; dialogs may hold it open for a while.
        store.consumePendingPromptLink()
        scope.launch { resolver.resolve(link) }
    }
}

private enum class PromptLinkChoice { Confirm, Neutral, Cancel }

private class PromptLinkResolver(
    private val store: AppStore,
    private val navController: NavHostController,
    private val dialogs: S5ConfirmDialogController,
) {
    suspend fun resolve(link: DeepLink) {
        when (link) {
            is DeepLink.NewTask -> link.promptQuery?.let { resolveNewThreadLink(it) }
            is DeepLink.Thread ->
                link.promptQuery?.let { query ->
                    when (val parsed = parseThreadLinkPrompt(query)) {
                        is PromptLinkResult.Error -> alert("Could not open link", parsed.error)
                        is PromptLinkResult.Ok ->
                            parsed.value?.let { prompt ->
                                applyToThreadDraft(link.environmentId, link.threadId, prompt)
                            }
                    }
                }
            else -> Unit
        }
    }

    private suspend fun resolveNewThreadLink(query: String) {
        val link =
            when (val parsed = parseNewThreadLink(query)) {
                is PromptLinkResult.Error -> return alert("Could not open link", parsed.error)
                is PromptLinkResult.Ok -> parsed.value
            }
        when (
            val plan =
                planNewThreadLink(link, store.workspace.threads.value, store.workspace.projects.value)
        ) {
            is NewThreadLinkPlan.OpenThread -> {
                val environmentId = plan.thread.environmentId.value
                val threadId = plan.thread.id.value
                navController.navigate(Routes.thread(environmentId, threadId)) {
                    popUpTo(Routes.NewTask) { inclusive = true }
                }
                plan.prompt?.let { applyToThreadDraft(environmentId, threadId, it) }
            }
            is NewThreadLinkPlan.OpenProject ->
                if (mayReplaceNewTaskDraft(plan.request.prompt)) openProjectDraft(plan.project, plan.request)
            is NewThreadLinkPlan.OpenScratch ->
                if (mayReplaceNewTaskDraft(plan.request.prompt)) openScratchDraft(plan.request)
            is NewThreadLinkPlan.MissingRepository -> {
                val request = plan.request
                val choice =
                    choose(
                        title = "You don't have ${plan.repository.fullName}",
                        confirmText = "Clone",
                        neutralText = "Use without project".takeIf { request.prompt != null },
                    )
                when (choice) {
                    PromptLinkChoice.Confirm -> cloneRepository(plan.repository, request)
                    PromptLinkChoice.Neutral ->
                        if (mayReplaceNewTaskDraft(request.prompt)) openScratchDraft(request)
                    PromptLinkChoice.Cancel -> Unit
                }
            }
        }
    }

    private suspend fun applyToThreadDraft(environmentId: String, threadId: String, prompt: LinkedPrompt) {
        val notice = promptLinkNotice(prompt.hiddenCharacters)
        when (linkedThreadDraftAction(store.threadDraft(environmentId, threadId).text, prompt.text)) {
            LinkedDraftAction.Fill -> {
                store.setThreadDraftWithContext(environmentId, threadId, prompt.text, emptyList())
                showNotice(notice)
            }
            LinkedDraftAction.Keep -> showNotice(notice)
            LinkedDraftAction.Ask -> {
                val choice =
                    choose(
                        title = "Linked prompt ready. Replace draft?",
                        message = notice,
                        confirmText = "Replace",
                        cancelText = "Keep mine",
                    )
                if (choice == PromptLinkChoice.Confirm) {
                    store.setThreadDraftWithContext(environmentId, threadId, prompt.text, emptyList())
                }
            }
        }
    }

    /**
     * RN gives every linked task a fresh draft; this client keeps a single
     * new-task draft, so what the user put there is not replaced without
     * asking. "Keep mine" leaves the draft and the link both untouched.
     */
    private suspend fun mayReplaceNewTaskDraft(prompt: LinkedPrompt?): Boolean {
        val draft = store.draft.value
        val action =
            linkedNewTaskDraftAction(
                draft.prompt,
                hasOtherContent = draft.attachments.isNotEmpty() || draft.contextRecords.isNotEmpty(),
                prompt = prompt?.text,
            )
        if (action != LinkedDraftAction.Ask) return true
        return choose(
            title = if (prompt != null) "Linked prompt ready. Replace draft?" else "Replace your new task draft?",
            message = prompt?.let { promptLinkNotice(it.hiddenCharacters) },
            confirmText = "Replace",
            cancelText = "Keep mine",
        ) == PromptLinkChoice.Confirm
    }

    private suspend fun openProjectDraft(project: Project, request: LinkedDraftRequest) {
        var branch = project.branch
        var workspaceMode = WorkspaceMode.CurrentCheckout
        var pullRequestReference: String? = null
        var pullRequestUnavailable: Int? = null
        request.pullRequest?.let { pullRequest ->
            // The draft checks the PR out in its own worktree; Send, not the
            // link, is what prepares it (`git.preparePullRequestThread`).
            val detail =
                try {
                    store.workspace.pullRequestDetail(
                        project.environmentId,
                        project.id,
                        pullRequest.repository.fullName,
                        pullRequest.number,
                    )
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    null
                }
            if (detail != null && detail.headBranch.isNotBlank()) {
                branch = detail.headBranch
                workspaceMode = WorkspaceMode.NewWorktree
                pullRequestReference = detail.url.takeIf(String::isNotBlank)
            } else {
                pullRequestUnavailable = pullRequest.number
            }
        }
        val selection = modelSelection(project.environmentId, request)
        // The caller asked before replacing anything, so this is a fresh draft.
        store.updateDraft(setsPullRequest = true) { draft ->
            draft.copy(
                environmentId = project.environmentId,
                projectKey = project.id.value,
                prompt = request.prompt?.text.orEmpty(),
                attachments = emptyList(),
                contextRecords = emptyList(),
                branch = branch,
                workspaceMode = workspaceMode,
                worktreePath = null,
                pullRequestReference = pullRequestReference,
                cloning = false,
                settings = draft.settings.withSelection(selection),
            )
        }
        navController.navigate(Routes.NewTaskDraft) { launchSingleTop = true }
        val modelUnavailable = request.provider != null && selection?.exact != true
        if (request.prompt != null || pullRequestUnavailable != null || modelUnavailable) {
            showNotice(
                promptLinkNotice(
                    hiddenCharacters = request.prompt?.hiddenCharacters ?: 0,
                    modelUnavailable = modelUnavailable,
                    pullRequestUnavailable = pullRequestUnavailable,
                )
            )
        }
    }

    private suspend fun openScratchDraft(request: LinkedDraftRequest) {
        val scratchEnvironments =
            store.workspace.environments.value
                .filter { it.state == ConnectionState.Connected && it.scratchWorkspaceRoot != null }
                .map { it.id }
        val environmentId =
            promptLinkScratchEnvironmentId(
                latestThreadEnvironmentIds(store.workspace.threads.value),
                scratchEnvironments,
            )
                ?: return alert(
                    "Could not start without a project",
                    "No connected environment offers threads without a project.",
                )
        val project =
            try {
                store.workspace.ensureScratchProject(environmentId)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                return alert(
                    "Could not start without a project",
                    error.message ?: "The folder for threads without a project could not be created.",
                )
            }
        openProjectDraft(project, request.copy(pullRequest = null))
    }

    /**
     * Opens the clone form for the repository. A plain prompt rides along in
     * the new-task draft, which the add-project flow binds to the cloned project
     * once it starts; a pull request's worktree needs the project first, so that
     * prompt waits for the link to be opened again.
     */
    private suspend fun cloneRepository(repository: PromptLinkRepository, request: LinkedDraftRequest) {
        val connected = store.workspace.environments.value.filter { it.state == ConnectionState.Connected }
        val environmentId =
            (connected.firstOrNull { it.id == store.draft.value.environmentId } ?: connected.firstOrNull())?.id
                ?: return alert("Could not clone", "Connect an environment first.")
        val carried = request.prompt?.takeIf { request.pullRequest == null }
        if (carried != null && !mayReplaceNewTaskDraft(carried)) return
        var modelUnavailable = false
        if (carried != null) {
            val selection = modelSelection(environmentId, request)
            modelUnavailable = request.provider != null && selection?.exact != true
            store.updateDraft {
                it.copy(
                    prompt = carried.text,
                    attachments = emptyList(),
                    contextRecords = emptyList(),
                    settings = it.settings.withSelection(selection),
                )
            }
        }
        navController.navigate(
            Routes.addProjectDestination(
                environmentId = environmentId.value,
                source = "url",
                remoteUrl = "https://github.com/${repository.fullName}.git",
                repositoryTitle = repository.fullName,
                repositoryName = repository.name,
            )
        )
        val prompt = request.prompt ?: return
        showNotice(
            listOf(
                    promptLinkNotice(prompt.hiddenCharacters, modelUnavailable = modelUnavailable),
                    if (carried != null) "The prompt opens in a new task once the clone starts."
                    else "Open the link again after the clone to get the prompt.",
                )
                .joinToString("\n\n")
        )
    }

    private fun modelSelection(environmentId: EnvironmentId, request: LinkedDraftRequest): PromptLinkModelSelection? =
        request.provider?.let {
            resolvePromptLinkModelSelection(store.workspace.serverProviders(environmentId), it, request.model)
        }

    private fun ThreadSettings.withSelection(selection: PromptLinkModelSelection?): ThreadSettings =
        selection?.let {
            copy(
                provider =
                    ProviderInstance(
                        instanceId = it.provider.instanceId,
                        driver = it.provider.driver,
                        displayName = it.provider.displayName,
                    ),
                model = it.model,
                options = emptyList(),
            )
        } ?: this

    private fun showNotice(notice: String) = alert("Prompt from a link", notice)

    private fun alert(title: String, message: String) {
        dialogs.show(
            S5ConfirmDialogRequest(
                title = title,
                message = message,
                confirmText = "OK",
                cancelText = null,
                onConfirm = {},
            )
        )
    }

    private suspend fun choose(
        title: String,
        confirmText: String,
        message: String? = null,
        cancelText: String = "Cancel",
        neutralText: String? = null,
    ): PromptLinkChoice =
        suspendCancellableCoroutine { continuation ->
            fun answer(choice: PromptLinkChoice) {
                if (continuation.isActive) continuation.resume(choice)
            }
            dialogs.show(
                S5ConfirmDialogRequest(
                    title = title,
                    message = message,
                    confirmText = confirmText,
                    cancelText = cancelText,
                    onConfirm = { answer(PromptLinkChoice.Confirm) },
                    onCancel = { answer(PromptLinkChoice.Cancel) },
                    neutralText = neutralText,
                    onNeutral = { answer(PromptLinkChoice.Neutral) },
                )
            )
        }
}
