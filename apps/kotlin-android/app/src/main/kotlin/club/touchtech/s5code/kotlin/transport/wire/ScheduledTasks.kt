package club.touchtech.s5code.kotlin.transport.wire

import kotlinx.serialization.Serializable

/**
 * Wire DTOs for the scheduled-task webhook APIs in
 * `packages/contracts/src/scheduledTask.ts`.
 *
 * The scheduled-task record itself stays decoded where it already lives — the
 * settings screen reads it as raw JSON — so this file only shapes what lands on
 * the new RPCs: the endpoint a task reports, the signature check it documents,
 * and the delivery log `listWebhookDeliveries`/`getWebhookDelivery` return.
 * Unions the contract calls open (`outcome`) decode as strings for the same
 * reason as the orchestration DTOs.
 */

/**
 * `ScheduledTaskWebhookSignature`: the HMAC-SHA256 check configured for a
 * webhook task. The read model carries only how to verify — never the secret.
 */
@Serializable
data class ScheduledTaskWebhookSignatureDto(
    /** Request header carrying the signature, e.g. `x-hub-signature-256`. */
    val header: String = "",
    /** hex | base64 — how the digest is encoded in the header. */
    val encoding: String = "",
    /** Text before the digest in the header value, e.g. `sha256=`; empty for none. */
    val prefix: String = "",
)

/**
 * `ScheduledTaskWebhookEndpoint`: where a webhook task receives requests.
 * [url] is the public T3 Connect URL; null when the environment is not linked,
 * leaving [path] — an environment-relative path that works on any origin that
 * reaches the environment — as the only shareable address.
 */
@Serializable
data class ScheduledTaskWebhookEndpointDto(
    val path: String = "",
    val url: String? = null,
    val hasSecret: Boolean = false,
)

/**
 * `ScheduledTaskWebhookDeliveryOutcome` is a closed literal set on the
 * contract, but a string here: a new outcome must not hide the row it lands on.
 */
@Serializable
data class ScheduledTaskWebhookDeliverySummaryDto(
    val id: String = "",
    val taskId: String = "",
    val receivedAt: String = "",
    val method: String = "",
    val contentType: String? = null,
    val bodyBytes: Long = 0,
    /** accepted | dispatch_failed | rejected_signature | disabled | rate_limited | expired */
    val outcome: String = "",
    /** True only when a configured signature matched; false also means "none configured". */
    val signatureVerified: Boolean = false,
    /** Template placeholders that had no value in this request and rendered empty. */
    val missingFields: List<String> = emptyList(),
    val error: String? = null,
)

/** `ScheduledTaskWebhookDelivery`: the summary plus the request itself, for the detail view. */
@Serializable
data class ScheduledTaskWebhookDeliveryDto(
    val id: String = "",
    val taskId: String = "",
    val receivedAt: String = "",
    val method: String = "",
    val contentType: String? = null,
    val bodyBytes: Long = 0,
    val outcome: String = "",
    val signatureVerified: Boolean = false,
    val missingFields: List<String> = emptyList(),
    val error: String? = null,
    val query: String = "",
    val headers: Map<String, String> = emptyMap(),
    /** UTF-8 body cut at the server's log limit; [bodyTruncated] marks the cut. */
    val body: String = "",
    val bodyTruncated: Boolean = false,
    /** The task's prompt after placeholder substitution; null on dispatch failures. */
    val renderedPrompt: String? = null,
)

@Serializable
data class ScheduledTaskListWebhookDeliveriesResultDto(
    val deliveries: List<ScheduledTaskWebhookDeliverySummaryDto> = emptyList(),
)

@Serializable
data class ScheduledTaskGetWebhookDeliveryResultDto(
    val delivery: ScheduledTaskWebhookDeliveryDto,
)
