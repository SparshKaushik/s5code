import * as NodeCrypto from "node:crypto";

import {
  RelayAgentActivityAggregateState,
  RelayAgentAwarenessPhase,
  type RelayDeliveryKind,
} from "@t3tools/contracts/relay";
import { stableStringify } from "@t3tools/shared/relaySigning";
import * as DateTime from "effect/DateTime";
import * as Option from "effect/Option";
import * as Redacted from "effect/Redacted";
import * as Schema from "effect/Schema";

const MAX_JOB_AGE_MS = 10 * 60 * 1_000;
export const DELIVERY_JOB_SIGNING_ALGORITHM = "hmac-sha256";

const DeliveryKindSchema = Schema.Literals([
  "live_activity_start",
  "live_activity_update",
  "live_activity_end",
  "android_live_update",
  "push_notification",
]);

export const DeliveryChannelSchema = Schema.Literals(["apns", "fcm"]);
export type DeliveryChannel = typeof DeliveryChannelSchema.Type;
const LiveActivityStartOrUpdateKindSchema = Schema.Literals([
  "live_activity_start",
  "live_activity_update",
]);
const LiveActivityKindSchema = Schema.Literals([
  "live_activity_start",
  "live_activity_update",
  "live_activity_end",
]);

const DeliveryJobContext = {
  jobId: Schema.String,
  userId: Schema.String,
  deviceId: Schema.String,
};

export const NotificationPayload = Schema.Struct({
  title: Schema.String,
  body: Schema.String,
  environmentId: Schema.String,
  threadId: Schema.String,
  deepLink: Schema.String,
  // Optional so delivery jobs queued by older relay builds still decode.
  // New jobs use these fields to avoid delivering a stale Done/attention
  // notification after the thread has moved to another phase.
  phase: Schema.optional(RelayAgentAwarenessPhase),
  updatedAt: Schema.optional(Schema.String),
});
export type NotificationPayload = typeof NotificationPayload.Type;

// Alert copy attached to a Live Activity update/end push. Its presence makes
// the update "alerting": iOS wakes the screen, plays the haptic, and briefly
// expands the Dynamic Island instead of silently redrawing.
export const LiveActivityAlert = Schema.Struct({
  title: Schema.String,
  body: Schema.String,
});
export type LiveActivityAlert = typeof LiveActivityAlert.Type;

export const DeliveryJobPayload = Schema.Struct({
  version: Schema.Literal(1),
  jobId: Schema.String,
  kind: DeliveryKindSchema,
  channel: DeliveryChannelSchema,
  target: Schema.Struct({
    userId: Schema.String,
    deviceId: Schema.String,
    token: Schema.String,
    // Per-device APNs routing; absent on jobs queued by older relay builds,
    // which fall back to the configured defaults.
    bundleId: Schema.optional(Schema.NullOr(Schema.String)),
    apsEnvironment: Schema.optional(Schema.NullOr(Schema.Literals(["sandbox", "production"]))),
  }),
  aggregate: Schema.NullOr(RelayAgentActivityAggregateState),
  notification: Schema.NullOr(NotificationPayload),
  // Android generations prevent delayed queue jobs from updating a newer
  // locally-armed surface. Optional for backward compatibility with old jobs.
  generationId: Schema.optional(Schema.String),
  // Optional so jobs queued by older relay builds still decode.
  alert: Schema.optional(Schema.NullOr(LiveActivityAlert)),
  createdAt: Schema.String,
  expiresAt: Schema.String,
});
export type DeliveryJobPayload = typeof DeliveryJobPayload.Type;

export const SignedDeliveryJob = Schema.Struct({
  algorithm: Schema.Literal(DELIVERY_JOB_SIGNING_ALGORITHM),
  payload: DeliveryJobPayload,
  signature: Schema.String,
});
export type SignedDeliveryJob = typeof SignedDeliveryJob.Type;

export class DeliveryJobQueuePayloadInvalid extends Schema.TaggedErrorClass<DeliveryJobQueuePayloadInvalid>()(
  "DeliveryJobQueuePayloadInvalid",
  {
    receivedType: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Invalid delivery queue job with ${this.receivedType} payload.`;
  }
}

export class DeliveryJobLiveActivityAggregateMissing extends Schema.TaggedErrorClass<DeliveryJobLiveActivityAggregateMissing>()(
  "DeliveryJobLiveActivityAggregateMissing",
  {
    ...DeliveryJobContext,
    kind: LiveActivityStartOrUpdateKindSchema,
  },
) {
  override get message(): string {
    return `${this.kind.replaceAll("_", " ")} job ${this.jobId} requires an aggregate.`;
  }
}

export class DeliveryJobLiveActivityNotificationUnexpected extends Schema.TaggedErrorClass<DeliveryJobLiveActivityNotificationUnexpected>()(
  "DeliveryJobLiveActivityNotificationUnexpected",
  {
    ...DeliveryJobContext,
    kind: LiveActivityKindSchema,
  },
) {
  override get message(): string {
    return `${this.kind.replaceAll("_", " ")} job ${this.jobId} must not carry a push notification payload.`;
  }
}

export class DeliveryJobAndroidLiveUpdateNotificationUnexpected extends Schema.TaggedErrorClass<DeliveryJobAndroidLiveUpdateNotificationUnexpected>()(
  "DeliveryJobAndroidLiveUpdateNotificationUnexpected",
  DeliveryJobContext,
) {
  override get message(): string {
    return `Android Live Update job ${this.jobId} must not carry a push notification payload.`;
  }
}

export class DeliveryJobPushNotificationMissing extends Schema.TaggedErrorClass<DeliveryJobPushNotificationMissing>()(
  "DeliveryJobPushNotificationMissing",
  DeliveryJobContext,
) {
  override get message(): string {
    return `push notification job ${this.jobId} requires a notification payload.`;
  }
}

export class DeliveryJobPushNotificationAggregateUnexpected extends Schema.TaggedErrorClass<DeliveryJobPushNotificationAggregateUnexpected>()(
  "DeliveryJobPushNotificationAggregateUnexpected",
  DeliveryJobContext,
) {
  override get message(): string {
    return `push notification job ${this.jobId} must not carry aggregate state.`;
  }
}

export class DeliveryJobCreatedAtInvalid extends Schema.TaggedErrorClass<DeliveryJobCreatedAtInvalid>()(
  "DeliveryJobCreatedAtInvalid",
  {
    ...DeliveryJobContext,
    kind: DeliveryKindSchema,
    createdAt: Schema.String,
  },
) {
  override get message(): string {
    return `delivery job ${this.jobId} has invalid creation time ${this.createdAt}.`;
  }
}

export class DeliveryJobExpiresAtInvalid extends Schema.TaggedErrorClass<DeliveryJobExpiresAtInvalid>()(
  "DeliveryJobExpiresAtInvalid",
  {
    ...DeliveryJobContext,
    kind: DeliveryKindSchema,
    expiresAt: Schema.String,
  },
) {
  override get message(): string {
    return `delivery job ${this.jobId} has invalid expiry ${this.expiresAt}.`;
  }
}

export class DeliveryJobTimeWindowInvalid extends Schema.TaggedErrorClass<DeliveryJobTimeWindowInvalid>()(
  "DeliveryJobTimeWindowInvalid",
  {
    ...DeliveryJobContext,
    kind: DeliveryKindSchema,
    createdAt: Schema.String,
    expiresAt: Schema.String,
  },
) {
  override get message(): string {
    return `delivery job ${this.jobId} has invalid time window ${this.createdAt} to ${this.expiresAt}.`;
  }
}

export class DeliveryJobTimeWindowTooLong extends Schema.TaggedErrorClass<DeliveryJobTimeWindowTooLong>()(
  "DeliveryJobTimeWindowTooLong",
  {
    ...DeliveryJobContext,
    kind: DeliveryKindSchema,
    createdAt: Schema.String,
    expiresAt: Schema.String,
  },
) {
  override get message(): string {
    return `delivery job ${this.jobId} time window ${this.createdAt} to ${this.expiresAt} is too long.`;
  }
}

export class DeliveryJobSignatureInvalid extends Schema.TaggedErrorClass<DeliveryJobSignatureInvalid>()(
  "DeliveryJobSignatureInvalid",
  {
    ...DeliveryJobContext,
    kind: DeliveryKindSchema,
  },
) {
  override get message(): string {
    return `Invalid signature for delivery job ${this.jobId}.`;
  }
}

export const DeliveryJobInvalid = Schema.Union([
  DeliveryJobQueuePayloadInvalid,
  DeliveryJobLiveActivityAggregateMissing,
  DeliveryJobLiveActivityNotificationUnexpected,
  DeliveryJobAndroidLiveUpdateNotificationUnexpected,
  DeliveryJobPushNotificationMissing,
  DeliveryJobPushNotificationAggregateUnexpected,
  DeliveryJobCreatedAtInvalid,
  DeliveryJobExpiresAtInvalid,
  DeliveryJobTimeWindowInvalid,
  DeliveryJobTimeWindowTooLong,
  DeliveryJobSignatureInvalid,
]);
export type DeliveryJobInvalid = typeof DeliveryJobInvalid.Type;

export class DeliveryJobExpired extends Schema.TaggedErrorClass<DeliveryJobExpired>()(
  "DeliveryJobExpired",
  {
    ...DeliveryJobContext,
    kind: DeliveryKindSchema,
    expiresAt: Schema.String,
  },
) {
  override get message(): string {
    return `delivery job ${this.jobId} expired at ${this.expiresAt}.`;
  }
}

export const DeliveryJobVerificationError = Schema.Union([DeliveryJobInvalid, DeliveryJobExpired]);
export type DeliveryJobVerificationError = typeof DeliveryJobVerificationError.Type;

export const isDeliveryJobVerificationError = Schema.is(DeliveryJobVerificationError);

export function makeDeliveryJobPayload(input: {
  readonly kind: RelayDeliveryKind;
  readonly channel: DeliveryChannel;
  readonly userId: string;
  readonly deviceId: string;
  readonly token: string;
  readonly bundleId?: string | null | undefined;
  readonly apsEnvironment?: "sandbox" | "production" | null | undefined;
  readonly aggregate: DeliveryJobPayload["aggregate"];
  readonly notification?: NotificationPayload | null;
  readonly generationId?: string | undefined;
  readonly alert?: LiveActivityAlert | null | undefined;
  readonly createdAt: string;
  readonly expiresAt: string;
  readonly jobId: string;
}): DeliveryJobPayload {
  return {
    version: 1,
    jobId: input.jobId,
    kind: input.kind,
    channel: input.channel,
    target: {
      userId: input.userId,
      deviceId: input.deviceId,
      token: input.token,
      ...(input.bundleId ? { bundleId: input.bundleId } : {}),
      ...(input.apsEnvironment ? { apsEnvironment: input.apsEnvironment } : {}),
    },
    aggregate: input.aggregate,
    notification: input.notification ?? null,
    ...(input.generationId ? { generationId: input.generationId } : {}),
    // Omitted (not null) when absent so signatures stay identical to jobs from
    // relay builds that predate the field.
    ...(input.alert ? { alert: input.alert } : {}),
    createdAt: input.createdAt,
    expiresAt: input.expiresAt,
  };
}

export function expiresAtForJob(createdAtMs: number): string {
  return DateTime.formatIso(Option.getOrThrow(DateTime.make(createdAtMs + MAX_JOB_AGE_MS)));
}

function validatePayloadShape(payload: DeliveryJobPayload): DeliveryJobInvalid | null {
  switch (payload.kind) {
    case "live_activity_start":
    case "live_activity_update":
      if (payload.aggregate === null) {
        return new DeliveryJobLiveActivityAggregateMissing({
          jobId: payload.jobId,
          kind: payload.kind,
          userId: payload.target.userId,
          deviceId: payload.target.deviceId,
        });
      }
      if (payload.notification !== null) {
        return new DeliveryJobLiveActivityNotificationUnexpected({
          jobId: payload.jobId,
          kind: payload.kind,
          userId: payload.target.userId,
          deviceId: payload.target.deviceId,
        });
      }
      return null;
    case "live_activity_end":
      if (payload.notification !== null) {
        return new DeliveryJobLiveActivityNotificationUnexpected({
          jobId: payload.jobId,
          kind: payload.kind,
          userId: payload.target.userId,
          deviceId: payload.target.deviceId,
        });
      }
      return null;
    case "android_live_update":
      if (!payload.generationId) {
        return new DeliveryJobQueuePayloadInvalid({
          receivedType: "android live update without generation",
          cause: "generationId is required",
        });
      }
      if (payload.notification !== null) {
        return new DeliveryJobAndroidLiveUpdateNotificationUnexpected({
          jobId: payload.jobId,
          userId: payload.target.userId,
          deviceId: payload.target.deviceId,
        });
      }
      return null;
    case "push_notification":
      if (payload.notification === null) {
        return new DeliveryJobPushNotificationMissing({
          jobId: payload.jobId,
          userId: payload.target.userId,
          deviceId: payload.target.deviceId,
        });
      }
      if (payload.aggregate !== null) {
        return new DeliveryJobPushNotificationAggregateUnexpected({
          jobId: payload.jobId,
          userId: payload.target.userId,
          deviceId: payload.target.deviceId,
        });
      }
      return null;
  }
}

function signatureForPayload(input: {
  readonly secret: Redacted.Redacted<string>;
  readonly payload: DeliveryJobPayload;
}): string {
  return NodeCrypto.createHmac("sha256", Redacted.value(input.secret))
    .update(stableStringify(input.payload))
    .digest("base64url");
}

function timingSafeEqualBase64Url(left: string, right: string): boolean {
  const leftBuffer = Buffer.from(left, "base64url");
  const rightBuffer = Buffer.from(right, "base64url");
  if (leftBuffer.length !== rightBuffer.length) {
    return false;
  }
  return NodeCrypto.timingSafeEqual(leftBuffer, rightBuffer);
}

export function signDeliveryJob(input: {
  readonly secret: Redacted.Redacted<string>;
  readonly payload: DeliveryJobPayload;
}): SignedDeliveryJob {
  return {
    algorithm: DELIVERY_JOB_SIGNING_ALGORITHM,
    payload: input.payload,
    signature: signatureForPayload(input),
  };
}

export function verifySignedDeliveryJob(input: {
  readonly secret: Redacted.Redacted<string>;
  readonly job: SignedDeliveryJob;
  readonly nowMs: number;
}): DeliveryJobPayload | DeliveryJobVerificationError {
  const payload = input.job.payload;
  const invalidPayload = validatePayloadShape(payload);
  if (invalidPayload !== null) {
    return invalidPayload;
  }
  const createdAt = DateTime.make(payload.createdAt);
  if (Option.isNone(createdAt)) {
    return new DeliveryJobCreatedAtInvalid({
      jobId: payload.jobId,
      kind: payload.kind,
      userId: payload.target.userId,
      deviceId: payload.target.deviceId,
      createdAt: payload.createdAt,
    });
  }
  const expiresAt = DateTime.make(payload.expiresAt);
  if (Option.isNone(expiresAt)) {
    return new DeliveryJobExpiresAtInvalid({
      jobId: payload.jobId,
      kind: payload.kind,
      userId: payload.target.userId,
      deviceId: payload.target.deviceId,
      expiresAt: payload.expiresAt,
    });
  }
  const createdAtMs = createdAt.value.epochMilliseconds;
  const expiresAtMs = expiresAt.value.epochMilliseconds;
  if (expiresAtMs <= createdAtMs) {
    return new DeliveryJobTimeWindowInvalid({
      jobId: payload.jobId,
      kind: payload.kind,
      userId: payload.target.userId,
      deviceId: payload.target.deviceId,
      createdAt: payload.createdAt,
      expiresAt: payload.expiresAt,
    });
  }
  if (expiresAtMs - createdAtMs > MAX_JOB_AGE_MS) {
    return new DeliveryJobTimeWindowTooLong({
      jobId: payload.jobId,
      kind: payload.kind,
      userId: payload.target.userId,
      deviceId: payload.target.deviceId,
      createdAt: payload.createdAt,
      expiresAt: payload.expiresAt,
    });
  }
  if (expiresAtMs <= input.nowMs) {
    return new DeliveryJobExpired({
      jobId: payload.jobId,
      kind: payload.kind,
      userId: payload.target.userId,
      deviceId: payload.target.deviceId,
      expiresAt: payload.expiresAt,
    });
  }
  const expected = signatureForPayload({
    secret: input.secret,
    payload,
  });
  if (!timingSafeEqualBase64Url(input.job.signature, expected)) {
    return new DeliveryJobSignatureInvalid({
      jobId: payload.jobId,
      kind: payload.kind,
      userId: payload.target.userId,
      deviceId: payload.target.deviceId,
    });
  }
  return payload;
}
