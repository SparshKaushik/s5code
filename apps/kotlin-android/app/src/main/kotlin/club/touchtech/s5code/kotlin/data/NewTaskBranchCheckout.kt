package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.BranchRef
import club.touchtech.s5code.kotlin.transport.wire.VcsListRefsResultDto

private const val VCS_REF_QUERY_MAX_LENGTH = 256

/**
 * `checkoutNewTaskBranch` from
 * `apps/mobile/src/features/threads/checkout-new-task-branch.ts`.
 *
 * [readRefs] lists local refs matching a query, from [cursor] (null asks for a
 * refreshed first page); [switchRef] checks a ref out in the project and
 * returns the branch now current.
 */
internal suspend fun checkoutNewTaskBranch(
    branch: BranchRef,
    newWorktree: Boolean,
    readRefs: suspend (query: String, cursor: Int?) -> VcsListRefsResultDto,
    switchRef: suspend (refName: String) -> String?,
): BranchRef {
    if (newWorktree || branch.current) {
        return if (branch.current) branch.copy(worktreePath = null) else branch
    }

    // Remote refs omit local worktrees, and historical thread metadata can be stale.
    val localName = if (branch.remote) deriveLocalBranchNameFromRemoteRef(branch.name) else branch.name
    var cursor: Int? = null
    while (true) {
        // Bound the substring query; compare the full name across every page.
        val refs = readRefs(localName.take(VCS_REF_QUERY_MAX_LENGTH), cursor)
        val local = refs.refs.firstOrNull { it.name == localName }
        if (local != null &&
            (!branch.remote || local.upstreamRef == "refs/remotes/${branch.name}") &&
            (local.current || local.worktreePath != null)
        ) {
            val found = BranchRef(
                name = local.name,
                current = local.current,
                remote = local.isRemote,
                ageLabel = branch.ageLabel,
                worktreePath = local.worktreePath,
            )
            if (local.current) return found.copy(worktreePath = null)
            when (refs.isAtRepositoryRoot) {
                null -> error(
                    "The server did not report whether this project is at the repository root, so this " +
                        "worktree cannot be reused safely. Update the server or choose another branch."
                )
                false -> error(
                    "This project is a repository subdirectory. Choose another branch or add the " +
                        "repository root as a project to reuse this worktree."
                )
                true -> return found
            }
        }
        if (local != null || refs.nextCursor == null) break
        cursor = refs.nextCursor
    }

    val refName = switchRef(branch.name)
    return branch.copy(
        name = refName ?: branch.name,
        current = true,
        remote = false,
        worktreePath = null,
    )
}

/**
 * `resolveNewTaskBranchWorktreePath`: the draft's existing worktree for a
 * picked branch, which is never the project checkout itself.
 */
internal fun newTaskBranchWorktreePath(newWorktree: Boolean, projectCwd: String, branchWorktreePath: String?): String? =
    if (newWorktree || branchWorktreePath.isNullOrEmpty() || branchWorktreePath == projectCwd) null
    else branchWorktreePath

// Picker refs carry no remote name, so a remote ref drops its first segment the
// way `deriveLocalBranchNameFromRemoteRef` in `packages/shared/src/git.ts` does.
private fun deriveLocalBranchNameFromRemoteRef(branchName: String): String {
    val separator = branchName.indexOf('/')
    if (separator <= 0 || separator == branchName.length - 1) return branchName
    return branchName.substring(separator + 1)
}
