package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.EnvironmentKind
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ProjectId
import club.touchtech.s5code.kotlin.model.ProviderInstance
import club.touchtech.s5code.kotlin.model.RepositoryIdentity
import club.touchtech.s5code.kotlin.model.ThreadId
import club.touchtech.s5code.kotlin.model.ThreadStatus
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderModelDto
import club.touchtech.s5code.kotlin.transport.wire.ThreadPullRequestLinkDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Prompt links come from strangers, so parsing is a trust boundary: ported from
 * `packages/shared/src/promptLinks.test.ts`, `packages/client-runtime/src/promptLinks.test.ts`
 * and `apps/mobile/src/features/prompt-links/prompt-link-model.test.ts`.
 */
class PromptLinksTest {

    @Test
    fun `removes characters a reader cannot see and counts them`() {
        val tagged = "rate me" + String(Character.toChars(0xE0041)) + String(Character.toChars(0xE0042))
        assertEquals(LinkedPrompt("abcrate me", 5), sanitizeLinkedPrompt("a​b‮c﻿$tagged"))
        assertEquals(LinkedPrompt("a\nb\nc", 0), sanitizeLinkedPrompt("a b c"))
    }

    @Test
    fun `keeps emoji sequences intact`() {
        val family = "👨‍👩‍👧"
        val heart = "❤️"
        assertEquals(LinkedPrompt("$family $heart", 0), sanitizeLinkedPrompt("$family $heart"))
        assertEquals(1, sanitizeLinkedPrompt("a‍b").hiddenCharacters)
        assertEquals(1, sanitizeLinkedPrompt("👨‍").hiddenCharacters)
        val cjk = String(Character.toChars(0x20000)) + "‍" + String(Character.toChars(0x20001))
        assertEquals(1, sanitizeLinkedPrompt(cjk).hiddenCharacters)
    }

    @Test
    fun `reads a shared prompt with its optional repository and model`() {
        assertEquals(
            PromptLinkResult.Ok(
                NewThreadLink.New(
                    prompt = LinkedPrompt("Rate me", 0),
                    repository = PromptLinkRepository("pingdotgg", "t3code"),
                    provider = "codex",
                    model = "gpt-5",
                )
            ),
            parseNewThreadLink(
                "?prompt=Rate+me&repo=https://github.com/pingdotgg/t3code.git&provider=Codex&model=gpt-5"
            ),
        )
    }

    @Test
    fun `reads a pull request link, whose prompt is optional`() {
        for (pr in listOf(
            "pingdotgg/t3code/42",
            "pingdotgg/t3code%2342",
            "https://github.com/pingdotgg/t3code/pull/42/files",
        )) {
            assertEquals(
                PromptLinkResult.Ok(
                    NewThreadLink.PullRequest(PromptLinkPullRequest("pingdotgg", "t3code", 42), null)
                ),
                parseNewThreadLink("?pr=$pr"),
            )
        }
    }

    @Test
    fun `refuses links it cannot open safely, with the upstream wording`() {
        assertEquals(PromptLinkResult.Error("This link has no prompt."), parseNewThreadLink("?repo=a/b"))
        assertEquals(
            PromptLinkResult.Error(
                "This prompt had only hidden characters, so it may be malicious. S5 Code did not open it."
            ),
            parseNewThreadLink("?prompt=%E2%80%8B"),
        )
        assertTrue(parseThreadLinkPrompt("?prompt=%E2%80%8B") is PromptLinkResult.Error)
        assertEquals(
            PromptLinkResult.Error("This link names a repository S5 Code cannot read."),
            parseNewThreadLink("?prompt=x&repo=../etc"),
        )
        assertEquals(
            PromptLinkResult.Error("This prompt is longer than 8,000 characters, so S5 Code did not open it."),
            parseNewThreadLink("?prompt=${"x".repeat(8_001)}"),
        )
        assertEquals(
            PromptLinkResult.Error("This link names a pull request S5 Code cannot read."),
            parseNewThreadLink("?pr=pingdotgg/t3code/0"),
        )
    }

    @Test
    fun `thread links read the prompt and ignore other parameters`() {
        assertEquals(PromptLinkResult.Ok(LinkedPrompt("Fix it", 0)), parseThreadLinkPrompt("?prompt=Fix+it"))
        assertEquals(PromptLinkResult.Ok(null), parseThreadLinkPrompt("?panel=diff"))
    }

    @Test
    fun `builds a link that parses back to the same prompt`() {
        val link = buildNewThreadLink("Swear count & rating? 100% ~ok")
        assertTrue(link.startsWith("https://app.s5code.touchtech.club/new?prompt="))
        val parsed = parseNewThreadLink(link.substringAfter('?'))
        assertEquals(
            "Swear count & rating? 100% ~ok",
            ((parsed as PromptLinkResult.Ok).value as NewThreadLink.New).prompt.text,
        )
    }

    @Test
    fun `query decoding tolerates malformed escapes and keeps the first value`() {
        assertEquals(
            mapOf("prompt" to "50%zz off", "repo" to "a/b"),
            promptLinkQueryParams("prompt=50%zz+off&repo=a/b&prompt=second"),
        )
        assertTrue(hasPromptLinkParams("?prompt=hi"))
        assertFalse(hasPromptLinkParams("?from=widget"))
    }

    private fun identity(canonicalKey: String, origin: String? = null) =
        RepositoryIdentity(canonicalKey = canonicalKey, originCanonicalKey = origin)

    private fun project(id: String, identity: RepositoryIdentity?, environment: String = "env") =
        Project(
            id = ProjectId(id),
            environmentId = EnvironmentId(environment),
            title = id,
            workspaceRoot = "/$id",
            repository = null,
            branch = "main",
            repositoryIdentity = identity,
        )

    private fun thread(
        id: String,
        updatedAt: Long,
        links: List<ThreadPullRequestLinkDto>? = null,
        environment: String = "env",
        projectId: String = "p",
    ) =
        ThreadSummary(
            id = ThreadId(id),
            environmentId = EnvironmentId(environment),
            projectId = ProjectId(projectId),
            title = id,
            status = ThreadStatus.Waiting,
            provider = ProviderInstance.Default,
            model = "m",
            branch = null,
            updatedLabel = "",
            updatedAtMillis = updatedAt,
            pullRequestLinks = links,
        )

    private fun link(number: Int, source: String = "created") =
        ThreadPullRequestLinkDto(
            host = "github.com",
            repository = "pingdotgg/t3code",
            number = number,
            url = "https://github.com/pingdotgg/t3code/pull/$number",
            source = source,
            linkedAt = "2026-01-01T00:00:00.000Z",
        )

    @Test
    fun `takes the first exact repository match and never a fork of it`() {
        val projects =
            listOf(
                project("fork", identity("github.com/pingdotgg/t3code", "github.com/me/t3code")),
                project("none", null),
                project("recent", identity("github.com/PingDotGG/T3Code")),
                project("older", identity("github.com/pingdotgg/t3code")),
            )
        assertEquals("recent", findPromptLinkProject(projects, PromptLinkRepository("pingdotgg", "t3code"))?.title)
        assertEquals("fork", findPromptLinkProject(projects, PromptLinkRepository("me", "t3code"))?.title)
        assertNull(findPromptLinkProject(projects, PromptLinkRepository("gitlab", "x")))
    }

    @Test
    fun `finds the most recently updated thread linked to the pull request`() {
        val threads =
            listOf(
                thread("old", 1, listOf(link(7))),
                thread("new", 2, listOf(link(7))),
                thread("other", 3, listOf(link(8))),
                thread("dismissed", 4, listOf(link(7, "stack-dismissed"))),
            )
        val pullRequest = PromptLinkPullRequest("PingDotGG", "t3code", 7)
        assertEquals("new", findPromptLinkPullRequestThread(threads, pullRequest)?.title)
        assertNull(findPromptLinkPullRequestThread(threads, pullRequest.copy(number = 9)))
    }

    private fun provider(instanceId: String, driver: String, status: String = "ready") =
        ServerProviderDto(
            instanceId = instanceId,
            driver = driver,
            enabled = true,
            status = status,
            models =
                listOf(
                    ServerProviderModelDto(slug = "small", name = "Small"),
                    ServerProviderModelDto(slug = "big", name = "Big", aliases = listOf("opus"), isDefault = true),
                ),
        )

    @Test
    fun `maps the requested provider and model onto a usable instance`() {
        val providers = listOf(provider("broken", "claudeAgent", "error"), provider("work", "claudeAgent"))
        val small = resolvePromptLinkModelSelection(providers, "claude", "small")
        assertEquals("work" to "small", small?.provider?.instanceId to small?.model)
        assertEquals(true, small?.exact)
        val alias = resolvePromptLinkModelSelection(providers, "claudeagent", "opus")
        assertEquals("work" to "big", alias?.provider?.instanceId to alias?.model)
        assertEquals(true, alias?.exact)
    }

    @Test
    fun `uses the instance default for an unknown model, and nothing for a missing provider`() {
        val providers = listOf(provider("codex", "codex"))
        val fallback = resolvePromptLinkModelSelection(providers, "codex", "nope")
        assertEquals("codex" to "big", fallback?.provider?.instanceId to fallback?.model)
        assertEquals(false, fallback?.exact)
        assertNull(resolvePromptLinkModelSelection(providers, "cursor", null))
    }

    @Test
    fun `notices replace the plain line with the hidden character warning`() {
        assertEquals("This prompt came from a link. Read it before you send.", promptLinkNotice(0))
        assertEquals(
            "This prompt had hidden characters, it may be malicious. They were removed. Read it before you send." +
                "\n\nThe requested model is not available, so your default is used.",
            promptLinkNotice(2, modelUnavailable = true),
        )
        assertEquals(
            "This prompt came from a link. Read it before you send." +
                "\n\nPull request #7 could not be loaded, so this task starts from your default branch.",
            promptLinkNotice(0, pullRequestUnavailable = 7),
        )
    }

    @Test
    fun `fills an empty composer and asks before replacing typed text`() {
        assertEquals(LinkedDraftAction.Fill, linkedThreadDraftAction("  \n", "Do it"))
        assertEquals(LinkedDraftAction.Keep, linkedThreadDraftAction("Do it", "Do it"))
        assertEquals(LinkedDraftAction.Ask, linkedThreadDraftAction("My notes", "Do it"))
    }

    @Test
    fun `a new-task link asks before replacing any draft content, prompt or not`() {
        assertEquals(LinkedDraftAction.Fill, linkedNewTaskDraftAction(" ", hasOtherContent = false, prompt = null))
        assertEquals(LinkedDraftAction.Fill, linkedNewTaskDraftAction("", hasOtherContent = false, prompt = "Do it"))
        assertEquals(LinkedDraftAction.Keep, linkedNewTaskDraftAction("Do it", hasOtherContent = false, prompt = "Do it"))
        // A pr= link without a prompt would still retarget the typed draft.
        assertEquals(LinkedDraftAction.Ask, linkedNewTaskDraftAction("My notes", hasOtherContent = false, prompt = null))
        // Attachments or context records alone count as content.
        assertEquals(LinkedDraftAction.Ask, linkedNewTaskDraftAction("", hasOtherContent = true, prompt = null))
        assertEquals(LinkedDraftAction.Ask, linkedNewTaskDraftAction("Do it", hasOtherContent = true, prompt = "Do it"))
    }

    @Test
    fun `prefers the most recently active machine that offers threads without a project`() {
        val (a, b, c) = listOf("a", "b", "c").map(::EnvironmentId)
        assertEquals(b, promptLinkScratchEnvironmentId(listOf(a, b, c), listOf(c, b)))
        assertEquals(c, promptLinkScratchEnvironmentId(listOf(a), listOf(c)))
        assertNull(promptLinkScratchEnvironmentId(listOf(a), emptyList()))
        assertEquals(
            listOf(EnvironmentId("a"), EnvironmentId("b")),
            latestThreadEnvironmentIds(
                listOf(thread("1", 2, environment = "a"), thread("2", 3, environment = "b"), thread("3", 4, environment = "a"))
            ),
        )
    }

    @Test
    fun `plans each link shape onto its destination`() {
        val prompt = LinkedPrompt("Go", 0)
        val repo = PromptLinkRepository("pingdotgg", "t3code")
        val older = project("older", identity("github.com/pingdotgg/t3code"))
        val recent = project("recent", identity("github.com/pingdotgg/t3code"), environment = "env-2")
        val threads =
            listOf(
                thread("linked", 5, listOf(link(7))),
                thread("activity", 9, environment = "env-2", projectId = "recent"),
            )
        val projects = listOf(older, recent)

        val linked = planNewThreadLink(
            NewThreadLink.PullRequest(PromptLinkPullRequest("pingdotgg", "t3code", 7), prompt), threads, projects,
        )
        assertEquals("linked", (linked as NewThreadLinkPlan.OpenThread).thread.title)

        val unlinked = planNewThreadLink(
            NewThreadLink.PullRequest(PromptLinkPullRequest("pingdotgg", "t3code", 8), null), threads, projects,
        )
        unlinked as NewThreadLinkPlan.OpenProject
        assertEquals("recent", unlinked.project.title)
        assertEquals(8, unlinked.request.pullRequest?.number)

        val noRepo = planNewThreadLink(NewThreadLink.New(prompt, null, "codex", null), threads, projects)
        assertEquals(LinkedDraftRequest(prompt, "codex", null, null), (noRepo as NewThreadLinkPlan.OpenScratch).request)

        val missing = planNewThreadLink(
            NewThreadLink.New(prompt, PromptLinkRepository("me", "other"), null, null), threads, projects,
        )
        assertEquals("me/other", (missing as NewThreadLinkPlan.MissingRepository).repository.fullName)
        assertEquals(
            "recent",
            (planNewThreadLink(NewThreadLink.New(prompt, repo, null, null), threads, projects) as NewThreadLinkPlan.OpenProject)
                .project.title,
        )
    }

    private fun environment(
        state: ConnectionState,
        snapshotLoaded: Boolean = false,
        lastError: String = "",
        enabled: Boolean = true,
    ) =
        Environment(
            id = EnvironmentId("env"),
            label = "env",
            host = "host",
            kind = EnvironmentKind.Direct,
            state = state,
            isEnabled = enabled,
            snapshotLoaded = snapshotLoaded,
            lastError = lastError,
        )

    @Test
    fun `waits for machines still connecting, not ones that settled offline`() {
        assertTrue(promptLinkWaitingForShells(listOf(environment(ConnectionState.Connecting))))
        assertTrue(promptLinkWaitingForShells(listOf(environment(ConnectionState.Connected))))
        assertFalse(promptLinkWaitingForShells(listOf(environment(ConnectionState.Connected, snapshotLoaded = true))))
        assertFalse(promptLinkWaitingForShells(listOf(environment(ConnectionState.Offline, lastError = "refused"))))
        assertFalse(promptLinkWaitingForShells(listOf(environment(ConnectionState.Connecting, enabled = false))))
        assertFalse(promptLinkWaitingForShells(emptyList()))
    }
}
