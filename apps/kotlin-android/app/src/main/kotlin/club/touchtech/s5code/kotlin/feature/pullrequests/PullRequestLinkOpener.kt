package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.data.ChangeRequestLinkCapabilities
import club.touchtech.s5code.kotlin.data.ChangeRequestLinkProject
import club.touchtech.s5code.kotlin.data.resolveChangeRequestLinkTarget

/**
 * `useOpenPullRequest`: a change request link opens its PR page when the
 * current page names an environment and project (a thread's, or a PR's own)
 * and that environment reads pull requests, borrowing another project on the
 * link's host if needed. Anywhere else the link is the browser's.
 */
@Composable
fun rememberPullRequestLinkOpener(store: AppStore, navController: NavHostController): (String) -> Boolean {
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val threads by store.workspace.threads.collectAsStateWithLifecycle()
    // Read at tap time so a capability push does not hand every link a new handler.
    val current by rememberUpdatedState(Triple(projects, environments, threads))
    return remember(navController) {
        { url ->
            val (projectList, environmentList, threadList) = current
            // The tapped link is on the current page, so its route says where it came from.
            val arguments = navController.currentBackStackEntry?.arguments
            val environmentId = arguments?.getString("environmentId")?.takeIf(String::isNotBlank)
            val threadId = arguments?.getString("threadId")?.takeIf(String::isNotBlank)
            val projectId =
                arguments?.getString("projectId")?.takeIf(String::isNotBlank)
                    ?: threadList.firstOrNull { it.environmentId.value == environmentId && it.id.value == threadId }?.projectId?.value
            val target =
                if (environmentId == null || projectId == null) null
                else resolveChangeRequestLinkTarget(
                    url = url,
                    environmentId = environmentId,
                    projectId = projectId,
                    projects =
                        projectList
                            .filter { !it.isScratch }
                            .map { ChangeRequestLinkProject(it.environmentId.value, it.id.value, it.repositoryIdentity) },
                    capabilities = { id ->
                        environmentList.firstOrNull { it.id.value == id && it.isEnabled }?.capabilities?.let {
                            ChangeRequestLinkCapabilities(it.pullRequests, it.threadPullRequests)
                        }
                    },
                )
            if (target != null) navController.navigate(Routes.pullRequest(target.environmentId, target.reference))
            target != null
        }
    }
}
