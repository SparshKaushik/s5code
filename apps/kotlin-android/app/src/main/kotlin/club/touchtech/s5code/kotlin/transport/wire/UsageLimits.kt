package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/**
 * `ServerProviderUsageWindow` — one rolling quota window a subscription
 * provider reports for the signed-in account. `id` is stable per provider so
 * sparse turn-driven updates land on the same row a full probe produced.
 */
@Serializable
data class UsageWindowDto(
    val id: String = "",
    /** session | weekly | monthly | other */
    val kind: String = "other",
    val label: String = "",
    val usedPercent: Double = 0.0,
    val resetsAt: String? = null,
    val windowDurationMins: Long? = null,
)

/** `ServerProviderResetCredits`: banked clears for a rate-limited account. */
@Serializable
data class UsageResetCreditsDto(
    val availableCount: Int = 0,
    val nextExpiresAt: String? = null,
    /** Pins redemption to the displayed credit, including retries elsewhere. */
    val nextCreditId: String? = null,
)

/**
 * `ServerProviderUsageLimits`: what the provider knows about the account's
 * subscription. `unavailable` separates "can never report" (API key) from
 * "the probe failed this time".
 */
@Serializable
data class UsageLimitsDto(
    val checkedAt: String = "",
    val windows: List<UsageWindowDto> = emptyList(),
    val resetCredits: UsageResetCreditsDto? = null,
    val unavailable: UsageLimitsUnavailableDto? = null,
)

@Serializable
data class UsageLimitsUnavailableDto(
    /** unsupported | probeFailed */
    val reason: String = "",
    val message: String? = null,
)

/** `UsageLimitSourceAccount`: one account a hub reports on. */
@Serializable
data class UsageLimitSourceAccountDto(
    val id: String = "",
    val driver: String = "",
    val email: String? = null,
    val plan: String? = null,
    val usageLimits: UsageLimitsDto = UsageLimitsDto(),
)

/**
 * `UsageLimitSourceSnapshot`: the published state of one configured
 * `usageLimitSources` entry. A source that could not be read keeps `error`
 * beside an empty account list rather than vanishing.
 */
@Serializable
data class UsageLimitSourceDto(
    val id: String = "",
    val kind: String = "",
    val label: String = "",
    val checkedAt: String = "",
    val accounts: List<UsageLimitSourceAccountDto> = emptyList(),
    val error: String? = null,
)

/** `ProviderConsumeResetCreditResult` — the redeem outcome strings. */
@Serializable
data class ConsumeResetCreditResultDto(
    /** reset | nothingToReset | noCredit | alreadyRedeemed */
    val outcome: String = "",
    val warning: String? = null,
)
