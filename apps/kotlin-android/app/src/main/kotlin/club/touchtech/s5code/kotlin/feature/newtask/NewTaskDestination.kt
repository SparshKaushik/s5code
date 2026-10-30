package club.touchtech.s5code.kotlin.feature.newtask

import androidx.navigation.NavController
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.app.Routes
import club.touchtech.s5code.kotlin.data.ThreadProjectRef
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.WorkspaceMode

/**
 * `newTaskDestination`: with "Use last project" on, New task starts in the open
 * thread's project, else the last one used here; null opens the picker.
 */
fun newTaskDestinationProject(
    projects: List<Project>,
    useLastProject: Boolean,
    current: ThreadProjectRef?,
    last: ThreadProjectRef?,
): Project? {
    if (!useLastProject) return null
    fun find(ref: ThreadProjectRef?) =
        ref?.let { projects.firstOrNull { project -> project.environmentId.value == it.environmentId && project.id.value == it.projectId } }
    return find(current) ?: find(last)
}

/**
 * New task from a button, keyboard shortcut or empty state. A remembered
 * project goes straight to the draft; the picker stays one tap away through the
 * draft's project control.
 */
fun NavController.openNewTask(store: AppStore) {
    val arguments = currentBackStackEntry?.arguments
    val project =
        store.newTaskStartProject(arguments?.getString("environmentId"), arguments?.getString("threadId"))
    if (project == null) {
        navigate(Routes.NewTask)
        return
    }
    store.updateDraft { draft ->
        // An unchanged project keeps the branch or worktree the draft already picked.
        if (draft.environmentId == project.environmentId && draft.projectKey == project.id.value) draft
        else
            draft.copy(
                environmentId = project.environmentId,
                projectKey = project.id.value,
                branch = project.branch,
                workspaceMode = WorkspaceMode.CurrentCheckout,
                worktreePath = null,
            )
    }
    navigate(Routes.NewTaskDraft) { launchSingleTop = true }
}
