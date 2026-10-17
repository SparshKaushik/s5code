package club.touchtech.s5code.kotlin.transport

/**
 * The RPC method names, copied from `WS_METHODS` and `ORCHESTRATION_WS_METHODS`
 * in `packages/contracts/src/rpc.ts`. Only the ones this client calls are listed;
 * an unused constant is a claim of support the UI does not back.
 */
internal object WsMethods {
    const val ServerGetConfig = "server.getConfig"
    const val ServerRefreshProviders = "server.refreshProviders"
    const val ProviderConsumeResetCredit = "provider.consumeResetCredit"
    const val ServerProbe = "server.probe"
    const val ServerGetUsageSummary = "server.getUsageSummary"
    const val ServerReportClientActivity = "server.reportClientActivity"
    const val SubscribeServerConfig = "subscribeServerConfig"

    const val OrchestrationSubscribeShell = "orchestration.subscribeShell"
    const val OrchestrationSubscribeThread = "orchestration.subscribeThread"
    const val OrchestrationDispatchCommand = "orchestration.dispatchCommand"
    const val OrchestrationSearchThreads = "orchestration.searchThreads"
    const val OrchestrationGetArchivedShellSnapshot = "orchestration.getArchivedShellSnapshot"
    const val OrchestrationGetThreadProjection = "orchestration.getThreadProjection"
    /** Full input/output for one withheld or summarized turn item. */
    const val OrchestrationGetTurnItem = "orchestration.getTurnItem"

    /**
     * `secrets.answerRequest` — the agent's private-secret card. Declared with
     * no success value, so callers use `execute`, not `request`.
     */
    const val SecretsAnswerRequest = "secrets.answerRequest"

    const val ScheduledTasksRotateWebhookToken = "scheduledTasks.rotateWebhookToken"
    const val ScheduledTasksListWebhookDeliveries = "scheduledTasks.listWebhookDeliveries"
    const val ScheduledTasksGetWebhookDelivery = "scheduledTasks.getWebhookDelivery"

    const val SubscribeWorktreeSetup = "subscribeWorktreeSetup"
    const val WorktreeSetupCancel = "worktreeSetup.cancel"

    const val VcsRefreshStatus = "vcs.refreshStatus"
    const val VcsListRefs = "vcs.listRefs"
    const val VcsCreateRef = "vcs.createRef"
    const val VcsSwitchRef = "vcs.switchRef"
    const val VcsPull = "vcs.pull"
    const val GitRunStackedAction = "git.runStackedAction"

    const val ProjectsListEntries = "projects.listEntries"
    const val ProjectsReadFile = "projects.readFile"
    const val ProjectsSearchEntries = "projects.searchEntries"

    const val PullRequestsList = "pullRequests.list"
    const val PullRequestsDetail = "pullRequests.detail"

    const val SubscribeDeviceState = "subscribeDeviceState"
    const val DeviceList = "device.list"
    const val DeviceShutdown = "device.shutdown"

    const val SubscribeProjectClones = "subscribeProjectClones"
    const val ProjectCloneStart = "projectClone.start"
    const val ProjectCloneCancel = "projectClone.cancel"
    const val ProjectCloneRetry = "projectClone.retry"

    const val AssetsCreateUrl = "assets.createUrl"
    const val AttachmentsCreateUploadUrl = "attachments.createUploadUrl"
    const val AttachmentsDelete = "attachments.delete"

    const val ReviewGetDiffPreview = "review.getDiffPreview"

    const val TerminalOpen = "terminal.open"
    const val TerminalAttach = "terminal.attach"
    const val TerminalWrite = "terminal.write"
    const val TerminalResize = "terminal.resize"
    const val TerminalClear = "terminal.clear"
    const val TerminalRestart = "terminal.restart"
    const val TerminalClose = "terminal.close"
    const val SubscribeTerminalMetadata = "subscribeTerminalMetadata"

    const val RewindGetStatus = "rewind.getStatus"
    const val RewindUndo = "rewind.undo"
    const val RewindRedo = "rewind.redo"

    const val FilesystemBrowse = "filesystem.browse"
    const val SourceControlLookupRepository = "sourceControl.lookupRepository"
    const val SourceControlCloneRepository = "sourceControl.cloneRepository"
    const val ServerDiscoverSourceControl = "server.discoverSourceControl"
    const val ServerUpdateSettings = "server.updateSettings"
    const val ServerUpdateServer = "server.updateServer"
    const val ServerUpdateProvider = "server.updateProvider"
}
