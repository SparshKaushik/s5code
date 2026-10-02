package club.touchtech.s5code.kotlin.data

import club.touchtech.s5code.kotlin.model.EnvironmentId
import club.touchtech.s5code.kotlin.model.LimitAccount
import club.touchtech.s5code.kotlin.model.LimitPool
import club.touchtech.s5code.kotlin.model.LimitPoolReset
import club.touchtech.s5code.kotlin.model.LimitPoolWindow
import club.touchtech.s5code.kotlin.model.ProviderUsageLimits
import club.touchtech.s5code.kotlin.model.ResetCreditTarget
import club.touchtech.s5code.kotlin.model.UsageLimitSource
import club.touchtech.s5code.kotlin.model.UsageLimitSourceAccount
import club.touchtech.s5code.kotlin.model.UsageLimitWindow
import club.touchtech.s5code.kotlin.model.UsageResetCredits
import club.touchtech.s5code.kotlin.transport.wire.ServerProviderDto
import club.touchtech.s5code.kotlin.transport.wire.UsageLimitSourceDto
import club.touchtech.s5code.kotlin.transport.wire.UsageLimitsDto
import club.touchtech.s5code.kotlin.transport.wire.UsageResetCreditsDto
import club.touchtech.s5code.kotlin.transport.wire.UsageWindowDto

/**
 * `usageLimits.ts` ported to the client model. The pooled view draws each
 * subscription account once across every environment and hub reporting it; the
 * composer's `/usage-limits` answer reads the same pools filtered to one
 * driver, so both surfaces agree on what is left.
 */

/** One environment's contribution to the pooled view: its label plus its live lists. */
data class LimitEnvironmentPresentation(
    val environmentId: EnvironmentId,
    val label: String,
    val providers: List<ServerProviderDto>,
    val sources: List<UsageLimitSourceDto>,
)

/* ── DTO → model ─────────────────────────────────────────────────────── */

private fun parseLimitInstant(value: String?): Long? = parseInstant(value)

private fun UsageWindowDto.toModel(): UsageLimitWindow =
    UsageLimitWindow(
        id = id,
        kind = kind,
        label = label,
        usedPercent = usedPercent.coerceIn(0.0, 100.0),
        resetsAtMillis = parseLimitInstant(resetsAt),
        windowDurationMins = windowDurationMins,
    )

private fun UsageResetCreditsDto.toModel(): UsageResetCredits =
    UsageResetCredits(
        availableCount = availableCount,
        nextExpiresAtMillis = parseLimitInstant(nextExpiresAt),
        nextCreditId = nextCreditId,
    )

private fun UsageLimitsDto.toModel(): ProviderUsageLimits =
    ProviderUsageLimits(
        checkedAtMillis = parseLimitInstant(checkedAt),
        windows = windows.map { it.toModel() },
        resetCredits = resetCredits?.toModel(),
        unavailableReason = unavailable?.reason,
        unavailableMessage = unavailable?.message,
    )

private fun UsageLimitSourceDto.toModel(): UsageLimitSource =
    UsageLimitSource(
        id = id,
        label = label,
        error = error,
        accounts =
            accounts.map { account ->
                UsageLimitSourceAccount(
                    id = account.id,
                    driver = account.driver,
                    email = account.email,
                    plan = account.plan,
                    limits = account.usageLimits.toModel(),
                )
            },
    )

/* ── `providersWithLimits` / `hasProviderUsageLimits` ────────────────── */

/** Enabled, installed, not unavailable, and actually reporting limits. */
private fun providersWithLimits(providers: List<ServerProviderDto>): List<ServerProviderDto> =
    providers.filter {
        it.enabled && it.installed &&
            it.availability != "unavailable" && it.usageLimits != null
    }

/**
 * Whether Limits has anything to say about [driver] — a provider row for it,
 * or a source carrying its accounts (or a failed source, which counts for
 * every driver rather than disappearing).
 */
fun hasProviderUsageLimits(
    driver: String,
    providers: List<ServerProviderDto>,
    sources: List<UsageLimitSourceDto>,
): Boolean =
    providersWithLimits(providers).any { it.driver == driver } ||
        sources.any { source ->
            source.accounts.any { it.driver == driver } ||
                (source.error != null && source.accounts.isEmpty())
        }

/** `/usage-limits` typed in full, exact match — arguments stay an ordinary prompt. */
fun isUsageLimitsCommand(prompt: String): Boolean =
    prompt.trim().equals("/usage-limits", ignoreCase = true)

/* ── accounts ─────────────────────────────────────────────────────────── */

private fun accountKey(driver: String, email: String?): String? =
    email?.trim()?.lowercase()?.takeIf { it.isNotEmpty() }?.let { "$driver:$it" }

/** `limitsNotice`: the one-line status under a provider heading. */
internal fun limitsNotice(limits: ProviderUsageLimits): String? =
    when (limits.unavailableReason) {
        "unsupported" -> limits.unavailableMessage ?: "This account has no subscription limits."
        "probeFailed" -> limits.unavailableMessage ?: "Could not read limits."
        else -> if (limits.windows.isEmpty()) "No limits reported." else null
    }

/**
 * `collectLimitAccounts`: the same email signed in natively on two
 * environments, or reported by a hub as well as natively, is one account — its
 * quota is one bucket. Credits and their redemption target follow the freshest
 * successful read; a hub redeem wins outright because only it clears the hub's
 * routing cooldown.
 */
fun collectLimitAccounts(presentations: List<LimitEnvironmentPresentation>): List<LimitAccount> {
    data class Pending(val environmentId: EnvironmentId, val account: LimitAccount)

    val accounts = LinkedHashMap<String, LimitAccount>()
    val creditSources = mutableMapOf<String, Pending>()
    val hubRedeems = mutableMapOf<String, Pending>()

    fun merge(key: String, next: Pending) {
        val nextAccount = next.account
        val nextChecked = nextAccount.limits.checkedAtMillis ?: Long.MIN_VALUE
        val previousHub = hubRedeems[key]
        if (nextAccount.redeem is ResetCreditTarget.Hub &&
            (previousHub == null ||
                nextChecked > (previousHub.account.limits.checkedAtMillis ?: Long.MIN_VALUE))
        ) {
            hubRedeems[key] = next
        }
        val previousCredit = creditSources[key]
        if (nextAccount.limits.resetCredits != null &&
            (previousCredit == null ||
                nextChecked > (previousCredit.account.limits.checkedAtMillis ?: Long.MIN_VALUE))
        ) {
            creditSources[key] = next
        }
        val previous = accounts[key]
        if (previous == null) {
            accounts[key] = nextAccount
            return
        }
        val previousChecked = previous.limits.checkedAtMillis ?: Long.MIN_VALUE
        val fresher = nextChecked > previousChecked
        // Two instances on one machine sharing an account still name it once.
        val environments =
            previous.environments +
                nextAccount.environments.filter { it !in previous.environments }
        val winner = if (fresher) nextAccount else previous
        // Credits and their redemption target travel together; a failed credit
        // probe must not erase a successful read from another environment.
        val creditSource = creditSources[key]
        accounts[key] =
            previous.copy(
                displayName = previous.displayName ?: nextAccount.displayName,
                plan = previous.plan ?: nextAccount.plan,
                accentColor = previous.accentColor ?: nextAccount.accentColor,
                environments = environments,
                // A hub only names the account when no environment has it natively.
                sourceLabel =
                    if (environments.isNotEmpty()) null
                    else previous.sourceLabel ?: nextAccount.sourceLabel,
                redeem =
                    hubRedeems[key]?.account?.redeem
                        ?: creditSource?.account?.redeem
                        ?: winner.redeem
                        ?: previous.redeem
                        ?: nextAccount.redeem,
                limits =
                    winner.limits.copy(
                        resetCredits = creditSource?.account?.limits?.resetCredits
                    ),
            )
    }

    for (presentation in presentations) {
        for (provider in providersWithLimits(presentation.providers)) {
            val limits = provider.usageLimits ?: continue
            val model = limits.toModel()
            if (limitsNotice(model) != null) continue
            merge(
                accountKey(provider.driver, provider.auth.email)
                    ?: "${presentation.environmentId.value}:${provider.instanceId}",
                Pending(
                    presentation.environmentId,
                    LimitAccount(
                        key = "${presentation.environmentId.value}:${provider.instanceId}",
                        driver = provider.driver,
                        displayName = provider.displayName?.trim()?.takeIf { it.isNotEmpty() },
                        email = provider.auth.email,
                        plan = provider.auth.label,
                        accentColor = provider.accentColor,
                        environments = listOf(presentation.label),
                        sourceLabel = null,
                        redeem =
                            ResetCreditTarget.Instance(
                                presentation.environmentId, provider.instanceId,
                            ),
                        limits = model,
                    ),
                ),
            )
        }
    }

    // Every hub account, including ones a native instance also knows: the hub
    // may hold the fresher read of the same subscription.
    val labelEnvironment = presentations.size > 1
    for (presentation in presentations) {
        for (sourceDto in presentation.sources) {
            val source = sourceDto.toModel()
            val sourceLabel =
                if (labelEnvironment) "${presentation.label} · ${source.label}" else source.label
            for (account in source.accounts) {
                if (limitsNotice(account.limits) != null) continue
                merge(
                    accountKey(account.driver, account.email) ?: "${source.id}:${account.id}",
                    Pending(
                        presentation.environmentId,
                        LimitAccount(
                            key = "${source.id}:${account.id}",
                            driver = account.driver,
                            displayName =
                                if (account.email == null) {
                                    account.id.removeSuffix(".json")
                                        .removeSuffix(".JSON")
                                        .takeIf { it.isNotEmpty() }
                                } else null,
                            email = account.email,
                            plan = account.plan,
                            accentColor = null,
                            environments = emptyList(),
                            sourceLabel = sourceLabel,
                            redeem =
                                account.limits.resetCredits?.nextCreditId?.let { creditId ->
                                    ResetCreditTarget.Hub(
                                        presentation.environmentId,
                                        sourceId = source.id,
                                        accountId = account.id,
                                        creditId = creditId,
                                    )
                                },
                            limits = account.limits,
                        ),
                    ),
                )
            }
        }
    }
    return accounts.values.toList()
}

/**
 * `collectLimitNotices`: what pooled bars cannot draw — a hub that failed to
 * read, a provider whose probe failed. Accounts that can never report are left
 * out; the environment is named only when more than one is connected.
 */
fun collectLimitNotices(presentations: List<LimitEnvironmentPresentation>): List<String> {
    fun label(environmentLabel: String, subject: String): String =
        if (presentations.size > 1) "$environmentLabel · $subject" else subject
    val notices = mutableListOf<String>()
    for (presentation in presentations) {
        for (provider in providersWithLimits(presentation.providers)) {
            val limits = provider.usageLimits ?: continue
            // An account that can never report (API key) is left out.
            if (limits.unavailable?.reason == "unsupported") continue
            val notice = limitsNotice(limits.toModel()) ?: continue
            val name = provider.displayName?.trim().takeUnless { it.isNullOrEmpty() }
                ?: provider.driver
            notices += "${label(presentation.label, name)}: $notice"
        }
        for (source in presentation.sources) {
            when {
                source.error != null ->
                    notices += "${label(presentation.label, source.label)}: ${source.error}"
                source.accounts.isEmpty() ->
                    notices += "${label(presentation.label, source.label)}: No accounts reported."
            }
        }
    }
    return notices
}

/* ── pooling ─────────────────────────────────────────────────────────── */

private const val MINUTE_MS = 60_000L
private const val HOUR_MS = 60 * MINUTE_MS
private const val DAY_MS = 24 * HOUR_MS

private val WINDOW_KIND_ORDER = mapOf("session" to 0, "weekly" to 1, "monthly" to 2, "other" to 3)

private fun resetMillis(window: UsageLimitWindow): Long? = window.resetsAtMillis

/** Elapsed share of the window, 0..1 — null when its clock is unknown. */
private fun elapsedShare(window: UsageLimitWindow, now: Long): Double? {
    val resetsAt = resetMillis(window) ?: return null
    val duration = window.windowDurationMins ?: return null
    val length = duration * MINUTE_MS
    if (length <= 0) return null
    return ((length - (resetsAt - now)).toDouble() / length).coerceIn(0.0, 1.0)
}

/** Usage against the clock: within five points of even spend counts on pace. */
private fun paceOfShares(usedPercent: Double, elapsed: Double): String {
    val gap = usedPercent - elapsed * 100
    return when {
        gap > 5 -> "ahead"
        gap < -5 -> "under"
        else -> "on"
    }
}

private fun accountSortName(account: LimitAccount): String =
    (account.displayName ?: account.email ?: account.key).lowercase()

private fun poolWindows(accounts: List<LimitAccount>, now: Long): List<LimitPoolWindow> {
    val byKey = LinkedHashMap<String, MutableList<Pair<LimitAccount, UsageLimitWindow>>>()
    for (account in accounts) {
        for (window in account.limits.windows) {
            byKey.getOrPut("${window.kind}:${window.id}") { mutableListOf() } += account to window
        }
    }
    return byKey.values
        .map { members ->
            val memberByAccount = members.associate { it.first.key to it }
            val first = members.first().second
            val usedPercent = members.sumOf { it.second.usedPercent } / members.size
            // Pace compares spend against the clock, judged only over members
            // that have a clock — a window with no reset would count as spend
            // with no time elapsed and skew the verdict.
            val timed =
                members.mapNotNull { (_, window) ->
                    elapsedShare(window, now)?.let { window.usedPercent to it }
                }
            val timedUsed = timed.map { it.first }.average().takeIf { timed.isNotEmpty() }
            val meanElapsed =
                if (timed.isNotEmpty()) timed.sumOf { it.second } / timed.size else null
            val resets =
                members
                    .mapNotNull { (account, window) ->
                        resetMillis(window)?.let { at ->
                            LimitPoolReset(
                                accountKey = account.key,
                                atMillis = at,
                                restoresPercent =
                                    Math.round(window.usedPercent / members.size).toInt(),
                            )
                        }
                    }
                    .sortedBy { it.atMillis }
            LimitPoolWindow(
                id = first.id,
                kind = first.kind,
                label = first.label,
                members = members,
                columns = accounts.map { account -> account to memberByAccount[account.key]?.second },
                usedPercent = Math.round(usedPercent).toInt(),
                remainingPercent = 100 - Math.round(usedPercent).toInt(),
                pace = meanElapsed?.let { paceOfShares(timedUsed ?: 0.0, it) },
                resets = resets,
            )
        }
        .sortedBy { WINDOW_KIND_ORDER[it.kind] ?: 3 }
}

/**
 * `collectLimitPools`: accounts grouped by driver, windows pooled by kind and
 * id — Codex's `primary` is a position, not a duration, so a monthly allowance
 * never averages into a five-hour pool.
 */
fun collectLimitPools(accounts: List<LimitAccount>, now: Long): List<LimitPool> =
    accounts
        .groupBy { it.driver }
        .map { (driver, members) ->
            val orderWindow =
                members
                    .flatMap { it.limits.windows }
                    .minByOrNull { WINDOW_KIND_ORDER[it.kind] ?: 3 }
            fun orderReset(account: LimitAccount): Long =
                account.limits.windows
                    .firstOrNull { it.kind == orderWindow?.kind && it.id == orderWindow?.id }
                    ?.let(::resetMillis)
                    ?: Long.MAX_VALUE
            val sorted =
                members.sortedWith(
                    compareBy<LimitAccount> { orderReset(it) }
                        .thenBy { accountSortName(it) }
                        .thenBy { it.key }
                )
            LimitPool(driver = driver, accounts = sorted, windows = poolWindows(sorted, now))
        }

// `CURSOR_USAGE_WINDOWS` order: the combined row first when it survives.
private val CURSOR_WINDOW_ORDER =
    listOf("totalPercentUsed", "autoPercentUsed", "apiPercentUsed")

/** Cursor's two usable pools instead of the combined row when both exist. */
fun displayLimitWindows(pool: LimitPool): List<LimitPoolWindow> {
    if (pool.driver != "cursor") return pool.windows
    val hasBoth =
        pool.windows.any { it.id == "autoPercentUsed" } &&
            pool.windows.any { it.id == "apiPercentUsed" }
    return pool.windows
        .filter { !hasBoth || it.id != "totalPercentUsed" }
        .sortedBy { cursorRank(it.id) }
}

private fun cursorRank(id: String): Int =
    CURSOR_WINDOW_ORDER.indexOf(id).let { if (it < 0) CURSOR_WINDOW_ORDER.size else it }

/* ── labels ──────────────────────────────────────────────────────────── */

/** `formatDuration`: `2h 13m`, `3d 4h`, `12m`. */
fun formatLimitDuration(ms: Long): String {
    val remaining = ms.coerceAtLeast(0)
    val days = remaining / DAY_MS
    val hours = remaining % DAY_MS / HOUR_MS
    val minutes = remaining % HOUR_MS / MINUTE_MS
    return when {
        days > 0 -> "${days}d ${hours}h"
        hours > 0 -> "${hours}h ${minutes}m"
        else -> "${minutes}m"
    }
}

/** `formatResetsIn`: `resets in 2h 13m`, or null without a reset. */
fun formatResetsIn(window: UsageLimitWindow, now: Long): String? {
    val resetsAt = resetMillis(window) ?: return null
    return if (resetsAt <= now) "resets now" else "resets in ${formatLimitDuration(resetsAt - now)}"
}
