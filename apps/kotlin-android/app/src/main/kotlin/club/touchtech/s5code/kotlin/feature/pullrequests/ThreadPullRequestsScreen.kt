package club.touchtech.s5code.kotlin.feature.pullrequests

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.CallMerge
import androidx.compose.material.icons.automirrored.rounded.MergeType
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.LinkOff
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Visibility
import androidx.compose.material.icons.rounded.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import club.touchtech.s5code.kotlin.app.AppStore
import club.touchtech.s5code.kotlin.data.PullRequestLinkInputResult
import club.touchtech.s5code.kotlin.data.PullRequestListLine
import club.touchtech.s5code.kotlin.data.PullRequestReference
import club.touchtech.s5code.kotlin.data.linkedPullRequestLines
import club.touchtech.s5code.kotlin.data.pullRequestLinkProject
import club.touchtech.s5code.kotlin.data.resolveLinkPullRequestInput
import club.touchtech.s5code.kotlin.data.toDto
import club.touchtech.s5code.kotlin.design.component.S5ConfirmDialogController
import club.touchtech.s5code.kotlin.design.component.S5EmptyState
import club.touchtech.s5code.kotlin.design.component.S5IconButton
import club.touchtech.s5code.kotlin.design.component.S5MenuOption
import club.touchtech.s5code.kotlin.design.component.S5OverflowMenu
import club.touchtech.s5code.kotlin.design.component.S5Screen
import club.touchtech.s5code.kotlin.design.component.S5TextField
import club.touchtech.s5code.kotlin.design.component.rememberClipboardWriter
import club.touchtech.s5code.kotlin.design.theme.S5Theme
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.ThreadDetail
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadPullRequestLink
import club.touchtech.s5code.kotlin.transport.parseChangeRequestUrl
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/**
 * RN's `ThreadPullRequestsSheet`: every pull request linked to a thread, stack
 * layers indented under their base, with link, watch and unlink controls.
 */
@Composable
fun ThreadPullRequestsScreen(
    store: AppStore,
    environmentId: String,
    threadId: String,
    confirmController: S5ConfirmDialogController,
    onBack: () -> Unit,
    onOpenPullRequest: (environmentId: String, reference: PullRequestReference) -> Unit,
) {
    val context = LocalContext.current
    val copy = rememberClipboardWriter()
    val scope = rememberCoroutineScope()
    val env = EnvironmentId(environmentId)
    val id = ThreadId(threadId)
    val capabilities = rememberEnvironmentCapabilities(store, environmentId)
    val linkCapable = capabilities.threadPullRequests
    val detail by remember(linkCapable, env, id) {
            if (linkCapable) store.workspace.thread(env, id) else flowOf<ThreadDetail?>(null)
        }
        .collectAsStateWithLifecycle(initialValue = null)
    val links = detail?.pullRequests.orEmpty()
    val lines = remember(links) { linkedPullRequestLines(links.filter { it.source != "stack-dismissed" }.map { it.toDto() }) }
    val byKey = remember(links) { links.associateBy { Triple(it.host, it.repository, it.number) } }
    val openCount = lines.count { it.link.snapshot == null || it.link.snapshot.state == "open" }
    val threads by store.workspace.threads.collectAsStateWithLifecycle()
    val projects by store.workspace.projects.collectAsStateWithLifecycle()
    val environments by store.workspace.environments.collectAsStateWithLifecycle()
    val project =
        remember(threads, projects, environmentId, threadId) {
            val projectId = threads.firstOrNull { it.environmentId == env && it.id == id }?.projectId
            projects.firstOrNull { it.environmentId == env && it.id == projectId }
        }
    var linkOpen by remember { mutableStateOf(false) }
    var linkInput by remember { mutableStateOf("") }
    var linking by remember { mutableStateOf(false) }
    var menuFor by remember { mutableStateOf<String?>(null) }

    fun openUrl(url: String) {
        runCatching { context.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, url.toUri())) }
            .onFailure { store.showError("The pull request could not be opened.") }
    }

    // A repository with a project in an environment that lists pull requests opens in-app, this one first.
    fun open(link: ThreadPullRequestLink) {
        val parsed = parseChangeRequestUrl(link.url)
        val key = parsed?.let { "${it.host}/${it.repository}".lowercase() }
        val candidates =
            projects.filter { candidate ->
                key != null && !candidate.isScratch &&
                    candidate.repositoryIdentity?.canonicalKey?.lowercase() == key &&
                    environments.any { it.id == candidate.environmentId && it.capabilities.pullRequests }
            }
        val target = candidates.firstOrNull { it.environmentId == env } ?: candidates.firstOrNull()
        if (target == null) {
            openUrl(link.url)
            return
        }
        onOpenPullRequest(
            target.environmentId.value,
            PullRequestReference(projectId = target.id.value, repository = link.repository, number = link.number, host = link.host),
        )
    }

    fun write(title: String, block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                confirmController.alert(title, failure.alertMessage())
            }
        }
    }

    fun submitLink() {
        val result =
            resolveLinkPullRequestInput(linkInput, pullRequestLinkProject(project?.repositoryIdentity)) { link ->
                val key = "${link.host}/${link.repository}".lowercase()
                projects.any { it.environmentId == env && it.repositoryIdentity?.canonicalKey?.lowercase() == key }
            }
        when (result) {
            null -> confirmController.alert("Couldn't link this pull request", "Use a pull request URL, 123, or #123.")
            is PullRequestLinkInputResult.Error -> confirmController.alert("Couldn't link this pull request", result.message)
            is PullRequestLinkInputResult.Link -> {
                linking = true
                scope.launch {
                    try {
                        val link = result.link
                        store.workspace.setPullRequestLinked(env, id, link.host, link.repository, link.number, link.url, linked = true)
                        linkOpen = false
                        linkInput = ""
                    } catch (cancelled: CancellationException) {
                        throw cancelled
                    } catch (failure: Exception) {
                        confirmController.alert("Couldn't link this pull request", failure.alertMessage())
                    } finally {
                        linking = false
                    }
                }
            }
        }
    }

    S5Screen(
        title = "Pull requests",
        subtitle = if (lines.isEmpty()) "" else "$openCount open · ${lines.size} linked",
        onBack = onBack,
        actions = {
            if (linkCapable) S5IconButton(icon = Icons.Rounded.Add, label = "Link pull request", onClick = { linkOpen = true })
        },
    ) { padding ->
        if (lines.isEmpty()) {
            S5EmptyState(
                icon = Icons.AutoMirrored.Rounded.CallMerge,
                title = "No linked pull requests.",
                detail =
                    if (linkCapable) "Link one with + to follow it from this thread."
                    else "This server can't link pull requests to threads.",
                modifier = Modifier.padding(padding),
            )
        } else {
            LazyColumn(
                Modifier.fillMaxSize().padding(padding),
                contentPadding = PaddingValues(vertical = S5Theme.spacing.small),
            ) {
                items(lines, key = { "${it.chainKey}:${it.link.host}/${it.link.repository}#${it.link.number}" }) { line ->
                    val link = byKey[Triple(line.link.host, line.link.repository, line.link.number)] ?: return@items
                    val rowKey = "${link.host}/${link.repository}#${link.number}"
                    LinkedPullRequestRow(
                        line = line,
                        watched = link.watched,
                        onClick = { open(link) },
                        menu = {
                            S5OverflowMenu(
                                icon = Icons.Rounded.MoreVert,
                                label = "Pull request actions",
                                options =
                                    buildList {
                                        add(S5MenuOption("copy", "Copy link", Icons.Rounded.ContentCopy))
                                        add(S5MenuOption("browser", "Open in browser", Icons.AutoMirrored.Rounded.OpenInNew))
                                        val open = link.snapshot == null || link.snapshot.state == "open"
                                        if (capabilities.threadPullRequestWatch && (open || link.watched)) {
                                            add(
                                                if (link.watched) S5MenuOption("watch", "Stop watching", Icons.Rounded.VisibilityOff)
                                                else S5MenuOption("watch", "Watch for changes", Icons.Rounded.Visibility)
                                            )
                                        }
                                        add(
                                            S5MenuOption(
                                                "unlink",
                                                if (link.source == "stack") "Dismiss from thread" else "Unlink from thread",
                                                Icons.Rounded.LinkOff,
                                                destructive = true,
                                            )
                                        )
                                    },
                                onSelect = { option ->
                                    when (option) {
                                        "copy" -> copy(link.url)
                                        "browser" -> openUrl(link.url)
                                        "watch" ->
                                            write("Couldn't change watching") {
                                                store.workspace.setPullRequestWatch(env, id, link, !link.watched)
                                            }
                                        "unlink" ->
                                            write("Couldn't unlink this pull request") {
                                                store.workspace.setPullRequestLinked(
                                                    env, id, link.host, link.repository, link.number, link.url, linked = false,
                                                )
                                            }
                                    }
                                },
                                expanded = menuFor == rowKey,
                                onExpandedChange = { menuFor = if (it) rowKey else null },
                            )
                        },
                    )
                }
            }
        }
    }

    if (linkOpen) {
        AlertDialog(
            onDismissRequest = { if (!linking) linkOpen = false },
            title = { Text("Link pull request") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(S5Theme.spacing.small)) {
                    Text(
                        "Paste a pull request URL, or enter 123 / #123 for this thread's repository.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    S5TextField(
                        value = linkInput,
                        onValueChange = { linkInput = it.take(2000) },
                        placeholder = "Pull request URL or #42",
                        singleLine = true,
                        enabled = !linking,
                    )
                }
            },
            dismissButton = { TextButton(enabled = !linking, onClick = { linkOpen = false }) { Text("Cancel") } },
            confirmButton = {
                TextButton(enabled = !linking && linkInput.isNotBlank(), onClick = ::submitLink) {
                    Text(if (linking) "Linking…" else "Link")
                }
            },
        )
    }
}

@Composable
private fun LinkedPullRequestRow(line: PullRequestListLine, watched: Boolean, onClick: () -> Unit, menu: @Composable () -> Unit) {
    val link = line.link
    val snapshot = link.snapshot
    val merged = snapshot?.state == "merged"
    Row(
        Modifier.fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(
                start = S5Theme.spacing.gutter + (minOf(line.depth, 3) * 16).dp,
                end = S5Theme.spacing.small,
                top = S5Theme.spacing.small,
                bottom = S5Theme.spacing.small,
            ),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(S5Theme.spacing.medium),
    ) {
        Icon(
            if (merged) Icons.AutoMirrored.Rounded.MergeType else Icons.AutoMirrored.Rounded.CallMerge,
            contentDescription = null,
            tint = if (merged) S5Theme.status.settled else MaterialTheme.colorScheme.primary,
            modifier = Modifier.size(20.dp),
        )
        Column(Modifier.weight(1f)) {
            Text(
                snapshot?.title ?: "Waiting for host state",
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                buildString {
                    append("${link.repository}#${link.number}")
                    snapshot?.let { append(" · ").append(if (it.isDraft && it.state == "open") "draft" else it.state) }
                    line.stack?.let { (_, size) -> append(" · $size-layer stack") }
                    if (watched) append(" · Watching")
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        menu()
    }
}
