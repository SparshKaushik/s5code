package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.transport.wire.PullRequestLabelDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListEntryDto
import club.touchtech.s5code.kotlin.transport.wire.PullRequestListResultDto
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/*
 * The Pull requests tab's pure logic: the typed-qualifier parser from
 * `packages/client-runtime/src/pullRequestQuery.ts` and the paging, merging and
 * filtering from `apps/mobile/src/features/pull-requests/pullRequestList.ts`.
 * Literals stay the contract's strings so a saved control and a wire value are
 * the same thing.
 */

/** `PullRequestListFilters`. Null means "not filtered on". */
@Serializable
data class PullRequestListFilters(
    /** only | hide */
    val draft: String? = null,
    /** approved | changes-requested | review-required | none */
    val review: String? = null,
    /** passing | failing */
    val checks: String? = null,
    /** Each group is one `label:` qualifier: every group must match, any name in a group does. */
    val labels: List<List<String>>? = null,
    val excludedLabels: List<String>? = null,
    val author: String? = null,
) {
    /** `{ ...this, ...over }`: each field [over] sets wins. */
    fun overriddenBy(over: PullRequestListFilters) =
        PullRequestListFilters(
            draft = over.draft ?: draft,
            review = over.review ?: review,
            checks = over.checks ?: checks,
            labels = over.labels ?: labels,
            excludedLabels = over.excludedLabels ?: excludedLabels,
            author = over.author ?: author,
        )

    fun toJson(): JsonObject = buildJsonObject {
        draft?.let { put("draft", it) }
        review?.let { put("review", it) }
        checks?.let { put("checks", it) }
        labels?.let { groups ->
            putJsonArray("labels") {
                groups.forEach { group -> add(kotlinx.serialization.json.JsonArray(group.map(::jsonString))) }
            }
        }
        excludedLabels?.let { names -> putJsonArray("excludedLabels") { names.forEach { add(jsonString(it)) } } }
        author?.let { put("author", it) }
    }
}

private fun jsonString(value: String) = kotlinx.serialization.json.JsonPrimitive(value)

/** RN `PullRequestListControls`, saved per environment scope. */
@Serializable
data class PullRequestListControls(
    /** all | open | closed | merged */
    val state: String = "open",
    /** all | reviewing | authored */
    val involvement: String = "all",
    val filters: PullRequestListFilters = PullRequestListFilters(),
    val projectId: String? = null,
    val projectEnvironmentId: String? = null,
    /** updated | created | oldest */
    val sort: String = "updated",
)

val DEFAULT_PR_LIST_CONTROLS = PullRequestListControls()

const val PR_LIST_PAGE_SIZE = 30
private const val PR_LIST_MAX_LIMIT = 500

/** `PullRequestListInput`, as one list target asks for it. */
data class PullRequestListInput(
    val state: String,
    val involvement: String? = null,
    val filters: PullRequestListFilters? = null,
    val projectId: String? = null,
    val limit: Int? = null,
    val cursors: Map<String, String>? = null,
    val query: String? = null,
) {
    fun toJson(): JsonObject = buildJsonObject {
        put("state", state)
        involvement?.let { put("involvement", it) }
        filters?.let { put("filters", it.toJson()) }
        projectId?.let { put("projectId", it) }
        limit?.let { put("limit", it) }
        cursors?.let { values ->
            // Sorted so one set of cursors is one cache key whatever order it arrived in.
            putJsonObject("cursors") { values.toSortedMap().forEach { (key, value) -> put(key, value) } }
        }
        query?.let { put("query", it) }
    }
}

/** `PullRequestRef`: one change request as the server addresses it. */
data class PullRequestReference(
    val projectId: String,
    val repository: String,
    val number: Int,
    val host: String? = null,
) {
    fun toJson(): JsonObject = buildJsonObject {
        put("projectId", projectId)
        host?.let { put("host", it) }
        put("repository", repository)
        put("number", number)
    }

    /** Stable cache identity: host is case-insensitive, an absent host is its own key. */
    val cacheKey: String
        get() = "$projectId ${host?.lowercase().orEmpty()} $repository#$number"
}

fun PullRequestListEntryDto.reference() = PullRequestReference(projectId, repository, number, host)

/* ── Typed qualifiers ────────────────────────────────────────────────── */

private val REVIEW_VALUES =
    mapOf(
        "approved" to "approved",
        "changes_requested" to "changes-requested",
        "changes-requested" to "changes-requested",
        "required" to "review-required",
        "review-required" to "review-required",
        "none" to "none",
    )
private val CHECKS_VALUES =
    mapOf("success" to "passing", "passing" to "passing", "failure" to "failing", "failing" to "failing")

private val QUERY_TOKEN = Regex("(?:[^\\s\"]|\"[^\"]*\")+")
private val QUALIFIER = Regex("^(-?)([A-Za-z][A-Za-z0-9_-]*):(.*)$")
private val LIST_PART = Regex("(?:[^,\"]|\"[^\"]*\")+")
private const val MAX_QUALIFIER_VALUES = 10
private const val MAX_QUALIFIER_LENGTH = 200

private fun qualifierValue(raw: String) = raw.replace("\"", "").trim()

private fun splitQualifierList(raw: String): List<String> =
    LIST_PART.findAll(raw).map { qualifierValue(it.value) }.filter { it.isNotEmpty() }.toList()

private fun boundedNames(names: List<String>): List<String> =
    names.take(MAX_QUALIFIER_VALUES).map { it.take(MAX_QUALIFIER_LENGTH).trim() }.filter { it.isNotEmpty() }

data class ParsedPullRequestQuery(val text: String, val filters: PullRequestListFilters)

/**
 * `parsePullRequestQuery`: GitHub-style qualifiers become filters, everything
 * else stays search text. An unknown `key:value` reads as a namespaced label
 * (`size:XXL`), which is what it almost always is.
 */
fun parsePullRequestQuery(raw: String): ParsedPullRequestQuery {
    val text = mutableListOf<String>()
    val labels = mutableListOf<List<String>>()
    val excluded = mutableListOf<String>()
    var author: String? = null
    var draft: String? = null
    var review: String? = null
    var checks: String? = null
    for (match in QUERY_TOKEN.findAll(raw)) {
        val token = match.value
        val qualifier = QUALIFIER.find(token)
        val rawValue = qualifier?.groupValues?.get(3).orEmpty()
        val value = if (qualifier == null) "" else qualifierValue(rawValue)
        val negated = qualifier?.groupValues?.get(1) == "-"
        val key = if (value.isEmpty()) "" else qualifier?.groupValues?.get(2)?.lowercase().orEmpty()
        val consumed =
            when (key) {
                "label" -> {
                    val names = boundedNames(splitQualifierList(rawValue))
                    if (names.isEmpty()) false
                    else {
                        if (negated) excluded += names else labels += names
                        true
                    }
                }
                "author" ->
                    if (negated) false
                    else {
                        author = value.take(MAX_QUALIFIER_LENGTH).trim()
                        true
                    }
                "draft" -> {
                    val lower = value.lowercase()
                    if (negated || (lower != "true" && lower != "false")) false
                    else {
                        draft = if (lower == "true") "only" else "hide"
                        true
                    }
                }
                "review" -> {
                    val decision = if (negated) null else REVIEW_VALUES[value.lowercase()]
                    if (decision == null) false
                    else {
                        review = decision
                        true
                    }
                }
                "status", "checks" -> {
                    val state = if (negated) null else CHECKS_VALUES[value.lowercase()]
                    if (state == null) false
                    else {
                        checks = state
                        true
                    }
                }
                "" -> false
                else ->
                    if (value.startsWith("/")) false
                    else {
                        val prefix = qualifier?.groupValues?.get(2).orEmpty()
                        val names =
                            boundedNames(
                                splitQualifierList(rawValue).map { name ->
                                    if (name.contains(':')) name else "$prefix:$name"
                                }
                            )
                        if (names.isEmpty()) false
                        else {
                            if (negated) excluded += names else labels += names
                            true
                        }
                    }
            }
        if (!consumed) text += token
    }
    return ParsedPullRequestQuery(
        text = text.joinToString(" "),
        filters =
            PullRequestListFilters(
                labels = labels.takeIf { it.isNotEmpty() }?.take(MAX_QUALIFIER_VALUES),
                excludedLabels = excluded.takeIf { it.isNotEmpty() }?.take(MAX_QUALIFIER_VALUES),
                author = author,
                draft = draft,
                review = review,
                checks = checks,
            ),
    )
}

data class PullRequestDraftFilterChange(val query: String, val filters: PullRequestListFilters)

/**
 * `applyPullRequestDraftFilterChange`: the Draft menu shows a typed `draft:`
 * qualifier, so picking another value takes the qualifier out of the query —
 * left in, it would keep outranking whatever the menu stores.
 */
fun applyPullRequestDraftFilterChange(
    query: String,
    menuFilters: PullRequestListFilters,
    next: PullRequestListFilters,
): PullRequestDraftFilterChange {
    val typedDraft = parsePullRequestQuery(query).filters.draft
    if (next.draft == (typedDraft ?: menuFilters.draft)) {
        return PullRequestDraftFilterChange(query, next.copy(draft = menuFilters.draft))
    }
    return PullRequestDraftFilterChange(
        query =
            if (typedDraft == null) query
            else
                QUERY_TOKEN.findAll(query)
                    .map { it.value }
                    .filter { parsePullRequestQuery(it).filters.draft == null }
                    .joinToString(" "),
        filters = next,
    )
}

/**
 * `resolvePullRequestAuthorFilter`: `author:me` / `@me` names whoever is signed
 * in to the host being read.
 */
fun resolvePullRequestAuthorFilter(author: String, viewer: String?): String {
    val trimmed = author.trim()
    if (!Regex("^@?me$", RegexOption.IGNORE_CASE).matches(trimmed)) return trimmed
    return if (viewer.isNullOrBlank()) trimmed else viewer
}

/* ── Pages ───────────────────────────────────────────────────────────── */

/** A list row with the environment it was read from; null only in tests of host-only logic. */
data class ScopedPullRequestEntry(val environmentId: String?, val entry: PullRequestListEntryDto)

/** `pullRequestListKey`: one change request whichever environment listed it. */
fun pullRequestListKey(entry: PullRequestListEntryDto) =
    "${entry.host.lowercase()}:${entry.repository.lowercase()}:${entry.number}"

/** `scopedPullRequestListKey`. */
fun scopedPullRequestListKey(row: ScopedPullRequestEntry) = "${row.environmentId}:${pullRequestListKey(row.entry)}"

/** Projects as the Scratch-scope check needs them. */
data class PullRequestProjectScope(val id: String, val environmentId: String, val isScratch: Boolean)

/**
 * `withoutScratchProjectScope`: a scope saved on the Scratch folder reads as
 * all projects, since Scratch has no repository to list. One whose projects
 * have not arrived yet is kept.
 */
fun withoutScratchProjectScope(
    controls: PullRequestListControls,
    projects: List<PullRequestProjectScope>,
): PullRequestListControls =
    if (projects.any { it.isScratch && it.id == controls.projectId && it.environmentId == controls.projectEnvironmentId }) {
        controls.copy(projectId = null, projectEnvironmentId = null)
    } else {
        controls
    }

/** RN `PullRequestListPages`: one target's loaded rows and where the next page starts. */
data class PullRequestListPages(
    val head: PullRequestListResultDto,
    val entries: List<PullRequestListEntryDto>,
    val nextCursors: Map<String, String>,
    val truncated: Boolean,
    val limit: Int,
    val continued: Boolean,
)

/** Newest `observedAt` wins a duplicate; rows sort newest-updated first. */
fun mergePullRequestListEntries(
    previous: List<PullRequestListEntryDto>,
    incoming: List<PullRequestListEntryDto>,
): List<PullRequestListEntryDto> {
    val entries = LinkedHashMap<String, PullRequestListEntryDto>()
    previous.forEach { entries[pullRequestListKey(it)] = it }
    for (entry in incoming) {
        val key = pullRequestListKey(entry)
        val held = entries[key]
        if (held == null || (entry.observedAt ?: 0.0) >= (held.observedAt ?: 0.0)) entries[key] = entry
    }
    return entries.values.sortedByDescending { it.updatedAt }
}

/**
 * `pullRequestListHead`: a refreshed first page. Once more pages are loaded the
 * head merges into them without rewinding their cursor.
 */
fun pullRequestListHead(previous: PullRequestListPages?, head: PullRequestListResultDto): PullRequestListPages {
    if (previous != null && previous.head === head) return previous
    return if (previous?.continued == true) {
        previous.copy(head = head, entries = mergePullRequestListEntries(previous.entries, head.entries))
    } else {
        PullRequestListPages(
            head = head,
            entries = head.entries,
            nextCursors = head.nextCursors,
            truncated = head.truncated,
            limit = PR_LIST_PAGE_SIZE,
            continued = false,
        )
    }
}

/** `pullRequestListInput`: typed qualifiers win over the saved menus; the rest is search text. */
fun pullRequestListInput(controls: PullRequestListControls, query: String): PullRequestListInput {
    val parsed = parsePullRequestQuery(query)
    return PullRequestListInput(
        state = controls.state,
        involvement = controls.involvement,
        filters = controls.filters.overriddenBy(parsed.filters),
        limit = PR_LIST_PAGE_SIZE,
        projectId = controls.projectId?.takeIf { it.isNotEmpty() },
        query = parsed.text.takeIf { it.isNotEmpty() },
    )
}

/**
 * `nextPullRequestListInput`: continue the repositories that have more; a
 * legacy listing with no cursors grows its limit up to the wire maximum.
 */
fun nextPullRequestListInput(pages: PullRequestListPages, input: PullRequestListInput): PullRequestListInput? {
    if (pages.nextCursors.isNotEmpty()) return input.copy(limit = PR_LIST_PAGE_SIZE, cursors = pages.nextCursors)
    return if (pages.truncated && pages.limit < PR_LIST_MAX_LIMIT) {
        input.copy(limit = minOf(pages.limit + PR_LIST_PAGE_SIZE, PR_LIST_MAX_LIMIT))
    } else {
        null
    }
}

fun appendPullRequestListPage(
    previous: PullRequestListPages,
    page: PullRequestListResultDto,
    input: PullRequestListInput,
) = previous.copy(
    entries = mergePullRequestListEntries(previous.entries, page.entries),
    nextCursors = page.nextCursors,
    truncated = page.truncated,
    limit = input.limit ?: PR_LIST_PAGE_SIZE,
    continued = true,
)

data class PullRequestListProjectError(
    val environmentId: String,
    val projectTitle: String,
    val message: String,
)

/** The all-environment listing: rows with their source, plus per-environment viewers. */
data class MergedPullRequestList(
    val entries: List<ScopedPullRequestEntry>,
    /** `"$environmentId $host"` → signed-in login. */
    val viewers: Map<String, String>,
    /** `"$environmentId $host"` for hosts that cannot search, so text filters locally. */
    val localSearchHosts: Set<String>,
    val errors: List<PullRequestListProjectError>,
    val continued: Boolean,
)

fun mergeEnvironmentPullRequestPages(sources: List<Pair<String, PullRequestListPages>>): MergedPullRequestList {
    val viewers = mutableMapOf<String, String>()
    val localSearchHosts = mutableSetOf<String>()
    val entries = mutableListOf<ScopedPullRequestEntry>()
    val errors = mutableListOf<PullRequestListProjectError>()
    for ((environmentId, pages) in sources) {
        pages.entries.mapTo(entries) { ScopedPullRequestEntry(environmentId, it) }
        pages.head.viewers.forEach { (host, viewer) -> viewers["$environmentId $host"] = viewer }
        pages.head.providers.filter { !it.searchesOnHost }.forEach { localSearchHosts += "$environmentId ${it.host}" }
        pages.head.errors.mapTo(errors) { PullRequestListProjectError(environmentId, it.projectTitle, it.message) }
    }
    return MergedPullRequestList(entries, viewers, localSearchHosts, errors, sources.any { it.second.continued })
}

data class PullRequestListFacets(val authors: List<String>, val labels: List<String>)

fun pullRequestListFacets(entries: List<PullRequestListEntryDto>) =
    PullRequestListFacets(
        authors = entries.mapNotNull { it.author?.login }.distinct().sorted(),
        labels = entries.flatMap { entry -> entry.labels.map { it.name } }.distinct().sorted(),
    )

/**
 * `filterPullRequestList`: the saved controls and typed qualifiers over the
 * loaded rows. A repository listed by several environments shows once,
 * preferring a [connectedEnvironmentIds] copy so opening it can load.
 */
fun filterPullRequestList(
    entries: List<ScopedPullRequestEntry>,
    controls: PullRequestListControls,
    viewers: Map<String, String>,
    query: String,
    localSearchHosts: Set<String>,
    connectedEnvironmentIds: Set<String>? = null,
): List<ScopedPullRequestEntry> {
    val parsed = parsePullRequestQuery(query)
    val words = parsed.text.lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }
    val filters = controls.filters.overriddenBy(parsed.filters)
    val filtered =
        entries.filter { row ->
            val entry = row.entry
            if (controls.state != "all" && entry.state != controls.state) return@filter false
            if (!controls.projectId.isNullOrEmpty() && entry.projectId != controls.projectId) return@filter false
            if (!controls.projectEnvironmentId.isNullOrEmpty() && row.environmentId != controls.projectEnvironmentId) {
                return@filter false
            }
            val viewerKey = row.environmentId?.let { "$it ${entry.host}" } ?: entry.host
            val viewer = viewers[viewerKey]
            val login = entry.author?.login.orEmpty().lowercase()
            if (controls.involvement == "reviewing" && !entry.viewerReviewRequested) return@filter false
            if (controls.involvement == "authored" && (viewer == null || login != viewer.lowercase())) {
                return@filter false
            }
            if ((filters.draft == "only" && !entry.isDraft) || (filters.draft == "hide" && entry.isDraft)) {
                return@filter false
            }
            if (!filters.author.isNullOrEmpty()) {
                if (login != resolvePullRequestAuthorFilter(filters.author, viewer).lowercase()) return@filter false
            }
            if (filters.review != null && entry.reviewDecision != null && entry.reviewDecision != filters.review) {
                return@filter false
            }
            if (filters.review == "none" && entry.reviewDecision != null) return@filter false
            if (filters.checks != null && entry.checksState != null && entry.checksState != filters.checks) {
                return@filter false
            }
            val labels = entry.labels.map { it.name.lowercase() }.toSet()
            if (filters.labels?.any { group -> group.none { it.lowercase() in labels } } == true) return@filter false
            if (filters.excludedLabels?.any { it.lowercase() in labels } == true) return@filter false
            if (words.isNotEmpty() && viewerKey in localSearchHosts) {
                val text =
                    "${entry.repository} #${entry.number} ${entry.title} ${entry.author?.login.orEmpty()} " +
                        "${entry.headBranch} ${entry.baseBranch} ${entry.labels.joinToString(" ") { it.name }}"
                val haystack = text.lowercase()
                if (!words.all { haystack.contains(it) }) return@filter false
            }
            true
        }
    fun reachable(row: ScopedPullRequestEntry) =
        connectedEnvironmentIds == null || (row.environmentId != null && row.environmentId in connectedEnvironmentIds)
    val unique = LinkedHashMap<String, ScopedPullRequestEntry>()
    for (row in filtered) {
        val key = pullRequestListKey(row.entry)
        val held = unique[key]
        if (held == null || (!reachable(held) && reachable(row))) unique[key] = row
    }
    return when (controls.sort) {
        "created" -> unique.values.sortedByDescending { it.entry.createdAt }
        "oldest" -> unique.values.sortedBy { it.entry.updatedAt }
        else -> unique.values.sortedByDescending { it.entry.updatedAt }
    }
}

/** How a row draws its review verdict; null when there is nothing to say. */
enum class PullRequestListReview(val label: String, val accessibleLabel: String) {
    Changes("Changes", "Changes requested"),
    Approved("Approved", "Reviews approved"),
    YourReview("Your review", "Your review requested"),
    Required("Review", "Review required"),
}

fun pullRequestListReviewStatus(reviewDecision: String?, viewerReviewRequested: Boolean): PullRequestListReview? =
    when {
        reviewDecision == "changes-requested" -> PullRequestListReview.Changes
        reviewDecision == "approved" -> PullRequestListReview.Approved
        viewerReviewRequested -> PullRequestListReview.YourReview
        reviewDecision == "review-required" -> PullRequestListReview.Required
        else -> null
    }

/** `pullRequestLabelColor`: the host's six-digit hex as an ARGB int, or null for anything else. */
fun pullRequestLabelColor(color: String?): Long? {
    val hex = color?.removePrefix("#") ?: return null
    if (!Regex("^[\\da-fA-F]{6}$").matches(hex)) return null
    return 0xFF000000 or hex.toLong(16)
}

/** `pullRequestLabelColors`: each label's color by lowercased name; the first usable color a name carries wins. */
fun pullRequestLabelColors(labels: List<PullRequestLabelDto>): Map<String, Long> {
    val colors = HashMap<String, Long>()
    for (label in labels) {
        val color = pullRequestLabelColor(label.color) ?: continue
        colors.putIfAbsent(label.name.lowercase(), color)
    }
    return colors
}

private const val FILTER_CHOICE_LIMIT = 100

/**
 * `pullRequestFilterChoices`: a filter picker's rows. What is picked stays in
 * view, then the loaded rows' values that match, then the host's answers, each
 * once. The host matched its own way (an author's display name, say), so its
 * answers are not narrowed again here.
 */
fun pullRequestFilterChoices(
    selected: List<String>,
    suggestions: List<String>,
    hosted: List<String>,
    query: String,
): List<String> {
    val needle = query.trim().lowercase()
    val seen = HashSet<String>()
    val rows = ArrayList<String>()
    fun add(value: String) {
        if (seen.add(value.lowercase())) rows += value
    }
    val loaded = suggestions.filter { it.lowercase().contains(needle) }
    selected.forEach(::add)
    // Loaded matches alone could fill the list, so the host's answers get room after the first half.
    loaded.take(FILTER_CHOICE_LIMIT / 2).forEach(::add)
    hosted.forEach(::add)
    loaded.drop(FILTER_CHOICE_LIMIT / 2).forEach(::add)
    return rows.take(FILTER_CHOICE_LIMIT)
}

/** Filter count for the Filters menu badge (RN `PullRequestListFilters`). */
fun pullRequestListFilterCount(controls: PullRequestListControls): Int =
    listOf(
            controls.state != "open",
            controls.involvement != "all",
            !controls.filters.author.isNullOrEmpty(),
            controls.filters.draft != null,
            controls.filters.review != null,
            controls.filters.checks != null,
            !controls.projectId.isNullOrEmpty(),
        )
        .count { it } + controls.filters.labels.orEmpty().flatten().size
