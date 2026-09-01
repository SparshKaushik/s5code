package club.touchtech.s5code.kotlin.transport

/**
 * The RPC method names, copied from `WS_METHODS` and `ORCHESTRATION_WS_METHODS`
 * in `packages/contracts/src/rpc.ts`. Only the ones this client calls are listed;
 * an unused constant is a claim of support the UI does not back.
 */
internal object WsMethods {
    const val ServerGetConfig = "server.getConfig"
    const val ServerProbe = "server.probe"
    const val ServerGetUsageSummary = "server.getUsageSummary"

    const val OrchestrationSubscribeShell = "orchestration.subscribeShell"
    const val OrchestrationSubscribeThread = "orchestration.subscribeThread"
    const val OrchestrationDispatchCommand = "orchestration.dispatchCommand"
    const val OrchestrationSearchThreads = "orchestration.searchThreads"
    const val OrchestrationGetArchivedShellSnapshot = "orchestration.getArchivedShellSnapshot"

    const val VcsRefreshStatus = "vcs.refreshStatus"
    const val VcsListRefs = "vcs.listRefs"
    const val VcsCreateRef = "vcs.createRef"
    const val VcsSwitchRef = "vcs.switchRef"
    const val VcsPull = "vcs.pull"
    const val GitRunStackedAction = "git.runStackedAction"

    const val ProjectsListEntries = "projects.listEntries"
    const val ProjectsReadFile = "projects.readFile"
    const val ProjectsSearchEntries = "projects.searchEntries"

    const val AssetsCreateUrl = "assets.createUrl"

    const val ReviewGetDiffPreview = "review.getDiffPreview"

    const val TerminalOpen = "terminal.open"
    const val TerminalAttach = "terminal.attach"
    const val TerminalWrite = "terminal.write"
    const val TerminalResize = "terminal.resize"
    const val TerminalClear = "terminal.clear"
    const val TerminalRestart = "terminal.restart"
    const val TerminalClose = "terminal.close"

    const val RewindGetStatus = "rewind.getStatus"
    const val RewindUndo = "rewind.undo"
    const val RewindRedo = "rewind.redo"

    const val FilesystemBrowse = "filesystem.browse"
    const val SourceControlLookupRepository = "sourceControl.lookupRepository"
    const val SourceControlCloneRepository = "sourceControl.cloneRepository"
}
