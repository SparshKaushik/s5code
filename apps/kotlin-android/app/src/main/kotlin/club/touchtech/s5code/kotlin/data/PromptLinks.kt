package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.Environment
import club.touchtech.s5code.kotlin.model.ConnectionState
import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.Project
import club.touchtech.s5code.kotlin.model.ThreadSummary
import club.touchtech.s5code.kotlin.transport.threadPullRequestKeyOf
import club.touchtech.s5code.kotlin.transport.threadPullRequestsOf
import club.touchtech.s5code.kotlin.transport.visibleThreadPullRequests
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto

/**
 * Prompt links open the app from outside it with a prompt ready to send. They
 * never send it: a link from a stranger fills a draft, and the user reads it and
 * presses Send. A link also cannot pick a runtime mode, branch, or worktree.
 *
 * Ported from `packages/shared/src/promptLinks.ts` (parsing),
 * `packages/client-runtime/src/promptLinks.ts` (matching), and
 * `apps/mobile/src/features/prompt-links/prompt-link-model.ts` (notices). On
 * Android the links arrive as `s5code://new?…` and
 * `s5code://threads/<environmentId>/<threadId>?prompt=…`; shared links are
 * https on the hosted app, which this client only builds, never receives.
 */

/** `DEFAULT_HOSTED_APP_URL` in `packages/shared/src/connectAuth.ts`. */
const val PROMPT_LINK_HOSTED_APP_URL = "https://app.s5code.touchtech.club"

/** URLs stay well under the limits browsers and link shorteners apply. */
const val PROMPT_LINK_MAX_PROMPT_LENGTH = 8_000

/** Query keys a new-task link reads; a `/new` link without any is a plain new task. */
val PROMPT_LINK_PARAM_KEYS = listOf("prompt", "repo", "pr", "provider", "model")

data class PromptLinkRepository(val owner: String, val name: String) {
    /** `owner/name`, as the missing-repository dialog and the clone URL spell it. */
    val fullName: String get() = "$owner/$name"
}

data class PromptLinkPullRequest(val owner: String, val name: String, val number: Int) {
    val repository: PromptLinkRepository get() = PromptLinkRepository(owner, name)
}

data class LinkedPrompt(val text: String, val hiddenCharacters: Int)

sealed interface NewThreadLink {
    data class New(
        val prompt: LinkedPrompt,
        val repository: PromptLinkRepository?,
        val provider: String?,
        val model: String?,
    ) : NewThreadLink

    data class PullRequest(val pullRequest: PromptLinkPullRequest, val prompt: LinkedPrompt?) :
        NewThreadLink
}

sealed interface PromptLinkResult<out T> {
    data class Ok<T>(val value: T) : PromptLinkResult<T>

    data class Error(val error: String) : PromptLinkResult<Nothing>
}

// Zero-width and bidi controls, tag and variation-selector blocks, and other
// characters that render as nothing. A model reads them; the person checking the
// draft cannot see them, which makes them the usual way to hide instructions.
private val HIDDEN_CHARACTERS =
    Regex(
        "[\\x{0000}-\\x{0008}\\x{000B}\\x{000C}\\x{000E}-\\x{001F}\\x{007F}-\\x{009F}" +
            "\\x{00AD}\\x{034F}\\x{061C}\\x{115F}\\x{1160}\\x{17B4}\\x{17B5}\\x{180B}-\\x{180F}" +
            "\\x{200B}\\x{200C}\\x{200E}\\x{200F}\\x{202A}-\\x{202E}\\x{2060}-\\x{206F}\\x{3164}" +
            "\\x{FE00}-\\x{FE0D}\\x{FEFF}\\x{FFA0}\\x{FFF0}-\\x{FFFB}\\x{1BCA0}-\\x{1BCA3}" +
            "\\x{1D173}-\\x{1D17A}\\x{E0000}-\\x{E0FFF}]"
    )

private val LINE_BREAKS = Regex("\r\n?|[  ]")

private const val ZERO_WIDTH_JOINER = 0x200D

/** Emoji and the pieces emoji sequences are built from, by numeric range. */
private fun isEmojiPart(codePoint: Int?): Boolean =
    codePoint != null &&
        (codePoint in 0x1F000..0x1FAFF || codePoint in 0x2190..0x2BFF || codePoint == 0xFE0F)

/** Strips invisible characters and counts them; a joiner survives only inside an emoji. */
fun sanitizeLinkedPrompt(raw: String): LinkedPrompt {
    var hiddenCharacters = 0
    val visible =
        LINE_BREAKS.replace(raw, "\n")
            .let { text -> HIDDEN_CHARACTERS.replace(text) { hiddenCharacters++; "" } }
            .codePoints()
            .toArray()
    val text = StringBuilder()
    visible.forEachIndexed { index, codePoint ->
        val kept =
            codePoint != ZERO_WIDTH_JOINER ||
                (isEmojiPart(visible.getOrNull(index - 1)) && isEmojiPart(visible.getOrNull(index + 1)))
        if (kept) text.appendCodePoint(codePoint) else hiddenCharacters++
    }
    return LinkedPrompt(text.toString().trim(), hiddenCharacters)
}

private val OWNER = Regex("[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})")
private val NAME = Regex("[A-Za-z0-9._-]{1,100}")
private val GIT_SUFFIX = Regex("\\.git$", RegexOption.IGNORE_CASE)
private val ONLY_DOTS = Regex("\\.+")

private fun repositoryOf(owner: String, name: String): PromptLinkRepository? {
    val trimmedName = name.replace(GIT_SUFFIX, "")
    if (!OWNER.matches(owner) || !NAME.matches(trimmedName) || ONLY_DOTS.matches(trimmedName)) {
        return null
    }
    return PromptLinkRepository(owner, trimmedName)
}

private val REPOSITORY_URL =
    Regex("https?://(?:www\\.)?github\\.com/([^/?#]+)/([^/?#]+)/?", RegexOption.IGNORE_CASE)
private val REPOSITORY_SHORT = Regex("([^/\\s]+)/([^/\\s]+)")

fun parsePromptLinkRepository(value: String): PromptLinkRepository? {
    val trimmed = value.trim()
    val match = REPOSITORY_URL.matchEntire(trimmed) ?: REPOSITORY_SHORT.matchEntire(trimmed)
    return match?.let { repositoryOf(it.groupValues[1], it.groupValues[2]) }
}

private val PULL_REQUEST_URL =
    Regex(
        "https?://(?:www\\.)?github\\.com/([^/?#]+)/([^/?#]+)/pull/(\\d+)(?:[/?#].*)?",
        RegexOption.IGNORE_CASE,
    )
private val PULL_REQUEST_SHORT = Regex("([^/\\s#]+)/([^/\\s#]+)(?:/|#)(\\d+)")

private fun parsePromptLinkPullRequest(value: String): PromptLinkPullRequest? {
    val trimmed = value.trim()
    val match = PULL_REQUEST_URL.matchEntire(trimmed) ?: PULL_REQUEST_SHORT.matchEntire(trimmed)
        ?: return null
    val repository = repositoryOf(match.groupValues[1], match.groupValues[2]) ?: return null
    val number = match.groupValues[3].toIntOrNull()?.takeIf { it > 0 } ?: return null
    return PromptLinkPullRequest(repository.owner, repository.name, number)
}

/**
 * `URLSearchParams` decoding: `+` is a space, percent escapes are UTF-8, and a
 * malformed escape stays literal instead of failing the whole link. The first
 * occurrence of a key wins, as `URLSearchParams.get` does.
 */
internal fun promptLinkQueryParams(query: String): Map<String, String> {
    val params = linkedMapOf<String, String>()
    query.removePrefix("?").split('&').forEach { pair ->
        if (pair.isEmpty()) return@forEach
        val key = decodeQueryComponent(pair.substringBefore('='))
        val value = if ('=' in pair) decodeQueryComponent(pair.substringAfter('=')) else ""
        params.putIfAbsent(key, value)
    }
    return params
}

private fun decodeQueryComponent(raw: String): String {
    val bytes = java.io.ByteArrayOutputStream()
    var index = 0
    while (index < raw.length) {
        val char = raw[index]
        val escaped =
            if (char == '%' && index + 2 < raw.length &&
                raw[index + 1].isHexDigit() && raw[index + 2].isHexDigit()
            ) {
                raw.substring(index + 1, index + 3).toInt(16)
            } else {
                null
            }
        when {
            escaped != null -> {
                bytes.write(escaped)
                index += 3
            }
            char == '+' -> {
                bytes.write(' '.code)
                index++
            }
            else -> {
                val end = raw.offsetByCodePoints(index, 1)
                bytes.write(raw.substring(index, end).toByteArray(Charsets.UTF_8))
                index = end
            }
        }
    }
    return bytes.toString(Charsets.UTF_8.name())
}

/** Whether an incoming query carries any prompt-link parameter. */
fun hasPromptLinkParams(query: String, keys: List<String> = PROMPT_LINK_PARAM_KEYS): Boolean {
    val params = promptLinkQueryParams(query)
    return keys.any(params::containsKey)
}

private fun readPrompt(params: Map<String, String>): PromptLinkResult<LinkedPrompt?> {
    val raw = params["prompt"] ?: return PromptLinkResult.Ok(null)
    if (raw.length > PROMPT_LINK_MAX_PROMPT_LENGTH) {
        return PromptLinkResult.Error(
            "This prompt is longer than ${"%,d".format(java.util.Locale.US, PROMPT_LINK_MAX_PROMPT_LENGTH)} characters, so S5 Code did not open it."
        )
    }
    val prompt = sanitizeLinkedPrompt(raw)
    if (prompt.text.isEmpty() && prompt.hiddenCharacters > 0) {
        return PromptLinkResult.Error(
            "This prompt had only hidden characters, so it may be malicious. S5 Code did not open it."
        )
    }
    return PromptLinkResult.Ok(prompt.takeIf { it.text.isNotEmpty() })
}

private fun optionalParam(params: Map<String, String>, key: String): String? =
    params[key]?.trim()?.ifEmpty { null }

fun parseNewThreadLink(query: String): PromptLinkResult<NewThreadLink> {
    val params = promptLinkQueryParams(query)
    val prompt = when (val read = readPrompt(params)) {
        is PromptLinkResult.Error -> return read
        is PromptLinkResult.Ok -> read.value
    }
    params["pr"]?.let { pr ->
        val pullRequest =
            parsePromptLinkPullRequest(pr)
                ?: return PromptLinkResult.Error("This link names a pull request S5 Code cannot read.")
        return PromptLinkResult.Ok(NewThreadLink.PullRequest(pullRequest, prompt))
    }
    if (prompt == null) return PromptLinkResult.Error("This link has no prompt.")
    val repo = params["repo"]
    val repository = repo?.let(::parsePromptLinkRepository)
    if (repo != null && repository == null) {
        return PromptLinkResult.Error("This link names a repository S5 Code cannot read.")
    }
    return PromptLinkResult.Ok(
        NewThreadLink.New(
            prompt = prompt,
            repository = repository,
            provider = optionalParam(params, "provider")?.lowercase(),
            model = optionalParam(params, "model"),
        )
    )
}

fun parseThreadLinkPrompt(query: String): PromptLinkResult<LinkedPrompt?> =
    readPrompt(promptLinkQueryParams(query))

/** The shareable https link for [prompt], encoded the way `URLSearchParams` does. */
fun buildNewThreadLink(prompt: String, origin: String = PROMPT_LINK_HOSTED_APP_URL): String =
    "$origin/new?prompt=${java.net.URLEncoder.encode(prompt, Charsets.UTF_8.name())}"

/* ── Resolution ──────────────────────────────────────────────────────── */

/** What a new-task link asks to put in the draft once a project is settled. */
data class LinkedDraftRequest(
    val prompt: LinkedPrompt?,
    val provider: String?,
    val model: String?,
    val pullRequest: PromptLinkPullRequest?,
)

sealed interface NewThreadLinkPlan {
    /** A thread already linked to the pull request; the prompt lands in its draft. */
    data class OpenThread(val thread: ThreadSummary, val prompt: LinkedPrompt?) : NewThreadLinkPlan

    data class OpenProject(val project: Project, val request: LinkedDraftRequest) : NewThreadLinkPlan

    /** No repository named: a draft in the "No project" workspace. */
    data class OpenScratch(val request: LinkedDraftRequest) : NewThreadLinkPlan

    data class MissingRepository(
        val repository: PromptLinkRepository,
        val request: LinkedDraftRequest,
    ) : NewThreadLinkPlan
}

/**
 * Where a parsed new-task link goes, following `usePromptLinkResolver`: a pull
 * request opens its newest linked thread, otherwise the most recently active
 * project of the named repository, otherwise the scratch workspace (no
 * repository) or the missing-repository question.
 */
fun planNewThreadLink(
    link: NewThreadLink,
    threads: List<ThreadSummary>,
    projects: List<Project>,
): NewThreadLinkPlan {
    val request =
        when (link) {
            is NewThreadLink.New -> LinkedDraftRequest(link.prompt, link.provider, link.model, null)
            is NewThreadLink.PullRequest -> LinkedDraftRequest(link.prompt, null, null, link.pullRequest)
        }
    if (link is NewThreadLink.PullRequest) {
        findPromptLinkPullRequestThread(threads, link.pullRequest)?.let {
            return NewThreadLinkPlan.OpenThread(it, link.prompt)
        }
    }
    val repository =
        when (link) {
            is NewThreadLink.New -> link.repository
            is NewThreadLink.PullRequest -> link.pullRequest.repository
        } ?: return NewThreadLinkPlan.OpenScratch(request)
    val lastActivity = mutableMapOf<String, Long>()
    threads.forEach { thread ->
        val key = "${thread.environmentId.value}:${thread.projectId.value}"
        if (thread.updatedAtMillis > (lastActivity[key] ?: Long.MIN_VALUE)) {
            lastActivity[key] = thread.updatedAtMillis
        }
    }
    val byActivity =
        projects
            .filterNot { it.isScratch }
            .sortedByDescending { lastActivity["${it.environmentId.value}:${it.id.value}"] ?: Long.MIN_VALUE }
    return findPromptLinkProject(byActivity, repository)
        ?.let { NewThreadLinkPlan.OpenProject(it, request) }
        ?: NewThreadLinkPlan.MissingRepository(repository, request)
}

// A link's `owner/name` is GitHub's. GitCafe syncs a repository two-way with GitHub under the
// same path, so a checkout cloned from git.cafe counts as the linked repository.
private val PROMPT_LINK_REPOSITORY_HOSTS = listOf("github.com", "git.cafe", "staging.git.cafe")

/** Callers pass projects most recently active first, so the user lands where they last worked. */
fun findPromptLinkProject(projects: List<Project>, repository: PromptLinkRepository): Project? {
    val path = "${repository.owner}/${repository.name}".lowercase()
    val keys = PROMPT_LINK_REPOSITORY_HOSTS.mapTo(hashSetOf()) { "$it/$path" }
    return projects.firstOrNull { it.repositoryIdentity?.groupingCanonicalKey?.lowercase() in keys }
}

/** The most recently updated thread with a visible link to [pullRequest]. */
fun findPromptLinkPullRequestThread(
    threads: List<ThreadSummary>,
    pullRequest: PromptLinkPullRequest,
): ThreadSummary? {
    val key = threadPullRequestKeyOf("github.com", "${pullRequest.owner}/${pullRequest.name}", pullRequest.number)
    var latest: ThreadSummary? = null
    for (thread in threads) {
        if (latest != null && thread.updatedAtMillis <= latest.updatedAtMillis) continue
        val linked =
            visibleThreadPullRequests(threadPullRequestsOf(thread.pullRequestLinks, thread.linkedPullRequest))
                .any { threadPullRequestKeyOf(it) == key }
        if (linked) latest = thread
    }
    return latest
}

/**
 * A cold launch from a link has no projects or threads yet, and matching then
 * would offer to clone a repository the user already has. A machine still
 * connecting may hold the repository or thread; one that keeps failing has
 * settled offline and must not hold every link.
 */
fun promptLinkWaitingForShells(environments: List<Environment>): Boolean =
    environments.any { environment ->
        environment.isEnabled &&
            when (environment.state) {
                ConnectionState.Connected -> !environment.snapshotLoaded
                ConnectionState.Connecting -> true
                ConnectionState.Recovering, ConnectionState.Offline -> environment.lastError.isBlank()
                else -> false
            }
    }

fun promptLinkScratchEnvironmentId(
    environmentIdsMostRecentFirst: List<EnvironmentId>,
    scratchEnvironmentIds: List<EnvironmentId>,
): EnvironmentId? =
    environmentIdsMostRecentFirst.firstOrNull { it in scratchEnvironmentIds }
        ?: scratchEnvironmentIds.firstOrNull()

/** Environments ordered by their most recently updated thread. */
fun latestThreadEnvironmentIds(threads: List<ThreadSummary>): List<EnvironmentId> {
    val latest = mutableMapOf<EnvironmentId, Long>()
    threads.forEach { thread ->
        if (thread.updatedAtMillis > (latest[thread.environmentId] ?: Long.MIN_VALUE)) {
            latest[thread.environmentId] = thread.updatedAtMillis
        }
    }
    return latest.entries.sortedByDescending { it.value }.map { it.key }
}

fun promptLinkNotice(
    hiddenCharacters: Int,
    modelUnavailable: Boolean = false,
    pullRequestUnavailable: Int? = null,
): String =
    listOfNotNull(
        if (hiddenCharacters > 0) {
            "This prompt had hidden characters, it may be malicious. They were removed. Read it before you send."
        } else {
            "This prompt came from a link. Read it before you send."
        },
        "The requested model is not available, so your default is used.".takeIf { modelUnavailable },
        pullRequestUnavailable?.let {
            "Pull request #$it could not be loaded, so this task starts from your default branch."
        },
    ).joinToString("\n\n")

enum class LinkedDraftAction { Fill, Keep, Ask }

/** An empty composer is filled; typed text is never replaced without asking. */
fun linkedThreadDraftAction(currentText: String, prompt: String): LinkedDraftAction =
    when {
        currentText.isBlank() -> LinkedDraftAction.Fill
        currentText == prompt -> LinkedDraftAction.Keep
        else -> LinkedDraftAction.Ask
    }

/**
 * A new-task link replaces the single new-task draft — its text, attachments,
 * `#` context and workspace — so anything there is asked about first, even when
 * the link carries no prompt of its own. [hasOtherContent] is attachments or
 * context records.
 */
fun linkedNewTaskDraftAction(currentText: String, hasOtherContent: Boolean, prompt: String?): LinkedDraftAction =
    when {
        hasOtherContent -> LinkedDraftAction.Ask
        currentText.isBlank() -> LinkedDraftAction.Fill
        prompt != null && currentText == prompt -> LinkedDraftAction.Keep
        else -> LinkedDraftAction.Ask
    }

/* ── Model selection ─────────────────────────────────────────────────── */

private val PROMPT_LINK_PROVIDER_ALIASES = mapOf("claude" to "claudeagent")

/** `MODEL_SLUG_ALIASES_BY_PROVIDER` in `packages/contracts/src/model.ts`. */
private val MODEL_SLUG_ALIASES_BY_DRIVER =
    mapOf(
        "codex" to
            mapOf(
                "gpt-5-codex" to "gpt-5.4",
                "5.4" to "gpt-5.4",
                "5.3" to "gpt-5.3-codex",
                "gpt-5.3" to "gpt-5.3-codex",
                "5.3-spark" to "gpt-5.3-codex-spark",
                "gpt-5.3-spark" to "gpt-5.3-codex-spark",
            ),
        "cursor" to
            mapOf(
                "composer" to "composer-2",
                "composer-1.5" to "composer-1.5",
                "composer-1" to "composer-1.5",
                "opus-4.6-thinking" to "claude-opus-4-6",
                "opus-4.6" to "claude-opus-4-6",
                "sonnet-4.6-thinking" to "claude-sonnet-4-6",
                "sonnet-4.6" to "claude-sonnet-4-6",
                "opus-4.5-thinking" to "claude-opus-4-5",
                "opus-4.5" to "claude-opus-4-5",
            ),
    )

/** `resolveSelectableModel` in `packages/shared/src/model.ts`: slug, name, alias, then slug alias. */
private fun resolveSelectableModel(provider: ServerProviderDto, value: String): String? {
    val trimmed = value.trim().ifEmpty { return null }
    val models = provider.models
    models.firstOrNull { it.slug == trimmed }?.let { return it.slug }
    models.firstOrNull { it.name.equals(trimmed, ignoreCase = true) }?.let { return it.slug }
    models.firstOrNull { model -> model.aliases.any { it.equals(trimmed, ignoreCase = true) } }
        ?.let { return it.slug }
    val normalized = MODEL_SLUG_ALIASES_BY_DRIVER[provider.driver]?.get(trimmed) ?: trimmed
    return models.firstOrNull { it.slug == normalized }?.slug
}

data class PromptLinkModelSelection(val provider: ServerProviderDto, val model: String, val exact: Boolean)

/**
 * `exact` is false when the instance exists but the model does not, so its
 * default model stands in. Null when no usable instance of that provider
 * exists; the user's default applies then.
 */
fun resolvePromptLinkModelSelection(
    providers: List<ServerProviderDto>,
    provider: String,
    model: String?,
): PromptLinkModelSelection? {
    val driver = PROMPT_LINK_PROVIDER_ALIASES[provider] ?: provider
    val usable =
        providers.filter {
            it.driver.lowercase() == driver && it.enabled && it.availability != "unavailable"
        }
    val instance = usable.firstOrNull { it.status == "ready" } ?: usable.firstOrNull { it.status == "warning" }
        ?: return null
    val requested = model?.let { resolveSelectableModel(instance, it) }
    val resolved =
        requested
            ?: (instance.models.firstOrNull { it.isDefault } ?: instance.models.firstOrNull())?.slug
            ?: return null
    return PromptLinkModelSelection(instance, resolved, exact = model == null || requested != null)
}

private fun Char.isHexDigit(): Boolean = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
