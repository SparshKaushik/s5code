import { describe, expect, it } from "@effect/vitest";
import { EnvironmentId, ThreadId } from "@t3tools/contracts";
import type { RelayAgentActivityAggregateState } from "@t3tools/contracts/relay";
import * as Redacted from "effect/Redacted";

import {
  makeDeliveryJobPayload,
  signDeliveryJob,
  verifySignedDeliveryJob,
} from "./deliveryJobs.ts";

const secret = Redacted.make("queue-signing-secret");
const aggregate: RelayAgentActivityAggregateState = {
  title: "T3 Code",
  subtitle: "Agent work in progress",
  activeCount: 1,
  updatedAt: "2026-05-25T00:00:00.000Z",
  activities: [
    {
      environmentId: EnvironmentId.make("env"),
      threadId: ThreadId.make("thread"),
      projectTitle: "Project",
      threadTitle: "Thread",
      modelTitle: "gpt-5.4",
      phase: "running",
      status: "Working",
      updatedAt: "2026-05-25T00:00:00.000Z",
      deepLink: "/threads/env/thread",
    },
  ],
};

const notification = {
  title: "Thread",
  body: "Input: Project",
  environmentId: "env",
  threadId: "thread",
  deepLink: "/threads/env/thread",
};

describe("deliveryJobs", () => {
  it("rejects tampered signed queue jobs", () => {
    const payload = makeDeliveryJobPayload({
      channel: "apns",
      kind: "live_activity_end",
      userId: "user-1",
      deviceId: "device-1",
      token: "token-1",
      aggregate: null,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-1",
    });
    const signed = signDeliveryJob({ secret, payload });
    const tampered = {
      ...signed,
      payload: {
        ...signed.payload,
        target: {
          ...signed.payload.target,
          token: "attacker-token",
        },
      },
    };

    const result = verifySignedDeliveryJob({
      secret,
      job: tampered,
      nowMs: 0,
    });

    expect(result).toMatchObject({
      _tag: "DeliveryJobSignatureInvalid",
      jobId: "job-1",
      kind: "live_activity_end",
      userId: "user-1",
      deviceId: "device-1",
      message: "Invalid signature for delivery job job-1.",
    });
  });

  it("rejects Live Activity start jobs without aggregate state", () => {
    const payload = makeDeliveryJobPayload({
      channel: "apns",
      kind: "live_activity_start",
      userId: "user-1",
      deviceId: "device-1",
      token: "token-1",
      aggregate: null,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-start-invalid",
    });
    const signed = signDeliveryJob({ secret, payload });

    const result = verifySignedDeliveryJob({
      secret,
      job: signed,
      nowMs: 0,
    });

    expect(result).toMatchObject({
      _tag: "DeliveryJobLiveActivityAggregateMissing",
      jobId: "job-start-invalid",
      kind: "live_activity_start",
      userId: "user-1",
      deviceId: "device-1",
      message: "live activity start job job-start-invalid requires an aggregate.",
    });
  });

  it("rejects push notification jobs carrying aggregate state", () => {
    const payload = makeDeliveryJobPayload({
      channel: "apns",
      kind: "push_notification",
      userId: "user-1",
      deviceId: "device-1",
      token: "token-1",
      aggregate,
      notification,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-push-invalid",
    });
    const signed = signDeliveryJob({ secret, payload });

    const result = verifySignedDeliveryJob({
      secret,
      job: signed,
      nowMs: 0,
    });

    expect(result).toMatchObject({
      _tag: "DeliveryJobPushNotificationAggregateUnexpected",
      jobId: "job-push-invalid",
      userId: "user-1",
      deviceId: "device-1",
      message: "push notification job job-push-invalid must not carry aggregate state.",
    });
  });

  it("accepts minimal kind-specific signed queue jobs", () => {
    const pushPayload = makeDeliveryJobPayload({
      channel: "apns",
      kind: "push_notification",
      userId: "user-1",
      deviceId: "device-1",
      token: "token-1",
      aggregate: null,
      notification,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-push-valid",
    });
    const liveActivityPayload = makeDeliveryJobPayload({
      channel: "apns",
      kind: "live_activity_update",
      userId: "user-1",
      deviceId: "device-1",
      token: "token-1",
      aggregate,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-live-valid",
    });

    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload: pushPayload }),
        nowMs: 0,
      }),
    ).toEqual(pushPayload);
    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload: liveActivityPayload }),
        nowMs: 0,
      }),
    ).toEqual(liveActivityPayload);
  });

  it("accepts Android Live Update update and end jobs", () => {
    const payload = makeDeliveryJobPayload({
      channel: "fcm",
      kind: "android_live_update",
      userId: "user-1",
      deviceId: "device-android",
      token: "fcm-token",
      generationId: "generation-1",
      aggregate,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-android-live",
    });
    const endPayload = { ...payload, jobId: "job-android-end", aggregate: null };

    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload }),
        nowMs: 0,
      }),
    ).toEqual(payload);
    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload: endPayload }),
        nowMs: 0,
      }),
    ).toEqual(endPayload);
  });

  it("rejects Android Live Update jobs without a generation", () => {
    const payload = makeDeliveryJobPayload({
      channel: "fcm",
      kind: "android_live_update",
      userId: "user-1",
      deviceId: "device-android",
      token: "fcm-token",
      aggregate,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:05:00.000Z",
      jobId: "job-android-invalid",
    });

    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload }),
        nowMs: 0,
      }),
    ).toMatchObject({
      _tag: "DeliveryJobQueuePayloadInvalid",
    });
  });

  it("rejects jobs with invalid or overlong time windows", () => {
    const basePayload = makeDeliveryJobPayload({
      channel: "apns",
      kind: "live_activity_end",
      userId: "user-1",
      deviceId: "device-1",
      token: "token-1",
      aggregate: null,
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:10:00.000Z",
      jobId: "job-window",
    });
    const invalidCreatedAt = {
      ...basePayload,
      createdAt: "not-a-date",
    };
    const invertedWindow = {
      ...basePayload,
      expiresAt: "2026-05-24T23:59:59.000Z",
    };
    const overlongWindow = {
      ...basePayload,
      expiresAt: "2026-05-25T00:10:01.000Z",
    };

    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload: invalidCreatedAt }),
        nowMs: 0,
      }),
    ).toMatchObject({
      _tag: "DeliveryJobCreatedAtInvalid",
      jobId: "job-window",
      kind: "live_activity_end",
      userId: "user-1",
      deviceId: "device-1",
      createdAt: "not-a-date",
      message: "delivery job job-window has invalid creation time not-a-date.",
    });
    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload: invertedWindow }),
        nowMs: 0,
      }),
    ).toMatchObject({
      _tag: "DeliveryJobTimeWindowInvalid",
      jobId: "job-window",
      kind: "live_activity_end",
      userId: "user-1",
      deviceId: "device-1",
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-24T23:59:59.000Z",
      message:
        "delivery job job-window has invalid time window 2026-05-25T00:00:00.000Z to 2026-05-24T23:59:59.000Z.",
    });
    expect(
      verifySignedDeliveryJob({
        secret,
        job: signDeliveryJob({ secret, payload: overlongWindow }),
        nowMs: 0,
      }),
    ).toMatchObject({
      _tag: "DeliveryJobTimeWindowTooLong",
      jobId: "job-window",
      kind: "live_activity_end",
      userId: "user-1",
      deviceId: "device-1",
      createdAt: "2026-05-25T00:00:00.000Z",
      expiresAt: "2026-05-25T00:10:01.000Z",
      message:
        "delivery job job-window time window 2026-05-25T00:00:00.000Z to 2026-05-25T00:10:01.000Z is too long.",
    });
  });
});
