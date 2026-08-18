import * as Alchemy from "alchemy";
import * as Cloudflare from "alchemy/Cloudflare";
import * as Crypto from "effect/Crypto";
import * as Context from "effect/Context";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Schema from "effect/Schema";

import {
  RelayDeliveryKind as RelayDeliveryKindSchema,
  type RelayDeliveryResult,
} from "@t3tools/contracts/relay";

import {
  sanitizeAgentActivityAggregateState,
  sanitizeNotificationPayload,
} from "./agentActivityPayloads.ts";
import {
  expiresAtForJob,
  makeDeliveryJobPayload,
  signDeliveryJob,
  type DeliveryChannel,
  type DeliveryJobPayload,
  type SignedDeliveryJob,
} from "./deliveryJobs.ts";
import * as RelayConfiguration from "../Config.ts";

export class DeliveryQueueSendError extends Schema.TaggedErrorClass<DeliveryQueueSendError>()(
  "DeliveryQueueSendError",
  {
    operation: Schema.Literals(["generate-job-id", "send"]),
    jobId: Schema.NullOr(Schema.String),
    kind: RelayDeliveryKindSchema,
    userId: Schema.String,
    deviceId: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to enqueue ${this.kind.replaceAll("_", " ")} delivery during ${this.operation} for device ${this.deviceId}.`;
  }
}

export type DeliveryQueueError = DeliveryQueueSendError;

export class DeliveryQueueSender extends Context.Service<
  DeliveryQueueSender,
  {
    readonly send: (body: SignedDeliveryJob) => Effect.Effect<void, Cloudflare.Queues.SendError>;
  }
>()("t3code-relay/agentActivity/DeliveryQueue/DeliveryQueueSender") {}

export class DeliveryQueue extends Context.Service<
  DeliveryQueue,
  {
    readonly enqueueLiveActivity: (input: {
      readonly kind: DeliveryJobPayload["kind"];
      readonly userId: string;
      readonly deviceId: string;
      readonly token: string;
      readonly bundleId?: string | null;
      readonly apsEnvironment?: "sandbox" | "production" | null;
      readonly aggregate: DeliveryJobPayload["aggregate"];
      readonly alert?: DeliveryJobPayload["alert"];
    }) => Effect.Effect<RelayDeliveryResult, DeliveryQueueError>;
    readonly enqueueAndroidLiveUpdate: (input: {
      readonly userId: string;
      readonly deviceId: string;
      readonly token: string;
      readonly generationId: string;
      readonly aggregate: DeliveryJobPayload["aggregate"];
    }) => Effect.Effect<RelayDeliveryResult, DeliveryQueueError>;
    readonly enqueuePushNotification: (input: {
      readonly channel: DeliveryChannel;
      readonly userId: string;
      readonly deviceId: string;
      readonly token: string;
      readonly bundleId?: string | null;
      readonly apsEnvironment?: "sandbox" | "production" | null;
      readonly notification: NonNullable<DeliveryJobPayload["notification"]>;
    }) => Effect.Effect<RelayDeliveryResult, DeliveryQueueError>;
  }
>()("t3code-relay/agentActivity/DeliveryQueue") {}

export const make = Effect.gen(function* () {
  const sender = yield* DeliveryQueueSender;
  const crypto = yield* Crypto.Crypto;
  const config = yield* RelayConfiguration.RelayConfiguration;

  return DeliveryQueue.of({
    enqueueLiveActivity: Effect.fn("relay.delivery_queue.enqueue_live_activity")(function* (input) {
      yield* Effect.annotateCurrentSpan({
        "relay.mobile.device_id": input.deviceId,
        "relay.delivery.kind": input.kind,
      });
      const now = yield* DateTime.now;
      const jobId = yield* crypto.randomUUIDv4.pipe(
        Effect.mapError(
          (cause) =>
            new DeliveryQueueSendError({
              operation: "generate-job-id",
              jobId: null,
              kind: input.kind,
              userId: input.userId,
              deviceId: input.deviceId,
              cause,
            }),
        ),
      );
      yield* Effect.annotateCurrentSpan({ "relay.delivery.job_id": jobId });
      const payload = makeDeliveryJobPayload({
        ...input,
        // Live Activities are iOS-only; the Android ongoing-notification
        // surface is a later phase and never rides this channel.
        channel: "apns",
        aggregate:
          input.aggregate === null ? null : sanitizeAgentActivityAggregateState(input.aggregate),
        jobId,
        createdAt: DateTime.formatIso(now),
        expiresAt: expiresAtForJob(now.epochMilliseconds),
      });
      const signed = signDeliveryJob({
        secret: config.deliveryJobSigningSecret,
        payload,
      });
      yield* sender.send(signed).pipe(
        Effect.mapError(
          (cause) =>
            new DeliveryQueueSendError({
              operation: "send",
              jobId,
              kind: input.kind,
              userId: input.userId,
              deviceId: input.deviceId,
              cause,
            }),
        ),
      );
      return {
        deviceId: input.deviceId,
        kind: input.kind,
        ok: true,
        queued: true,
        deliveryStatus: null,
        deliveryReason: null,
        providerMessageId: null,
      };
    }),
    enqueueAndroidLiveUpdate: Effect.fn("relay.delivery_queue.enqueue_android_live_update")(
      function* (input) {
        const now = yield* DateTime.now;
        const jobId = yield* crypto.randomUUIDv4.pipe(
          Effect.mapError(
            (cause) =>
              new DeliveryQueueSendError({
                operation: "generate-job-id",
                jobId: null,
                kind: "android_live_update",
                userId: input.userId,
                deviceId: input.deviceId,
                cause,
              }),
          ),
        );
        const payload = makeDeliveryJobPayload({
          kind: "android_live_update",
          channel: "fcm",
          userId: input.userId,
          deviceId: input.deviceId,
          token: input.token,
          generationId: input.generationId,
          aggregate:
            input.aggregate === null ? null : sanitizeAgentActivityAggregateState(input.aggregate),
          notification: null,
          jobId,
          createdAt: DateTime.formatIso(now),
          expiresAt: expiresAtForJob(now.epochMilliseconds),
        });
        yield* sender
          .send(signDeliveryJob({ secret: config.deliveryJobSigningSecret, payload }))
          .pipe(
            Effect.mapError(
              (cause) =>
                new DeliveryQueueSendError({
                  operation: "send",
                  jobId,
                  kind: "android_live_update",
                  userId: input.userId,
                  deviceId: input.deviceId,
                  cause,
                }),
            ),
          );
        return {
          deviceId: input.deviceId,
          kind: "android_live_update" as const,
          ok: true,
          queued: true,
          deliveryStatus: null,
          deliveryReason: null,
          providerMessageId: null,
        };
      },
    ),
    enqueuePushNotification: Effect.fn("relay.delivery_queue.enqueue_push_notification")(
      function* (input) {
        yield* Effect.annotateCurrentSpan({
          "relay.mobile.device_id": input.deviceId,
          "relay.delivery.kind": "push_notification",
          "relay.environment_id": input.notification.environmentId,
          "relay.thread_id": input.notification.threadId,
        });
        const now = yield* DateTime.now;
        const jobId = yield* crypto.randomUUIDv4.pipe(
          Effect.mapError(
            (cause) =>
              new DeliveryQueueSendError({
                operation: "generate-job-id",
                jobId: null,
                kind: "push_notification",
                userId: input.userId,
                deviceId: input.deviceId,
                cause,
              }),
          ),
        );
        yield* Effect.annotateCurrentSpan({ "relay.delivery.job_id": jobId });
        const payload = makeDeliveryJobPayload({
          kind: "push_notification",
          channel: input.channel,
          userId: input.userId,
          deviceId: input.deviceId,
          token: input.token,
          bundleId: input.bundleId,
          apsEnvironment: input.apsEnvironment,
          aggregate: null,
          notification: sanitizeNotificationPayload(input.notification),
          jobId,
          createdAt: DateTime.formatIso(now),
          expiresAt: expiresAtForJob(now.epochMilliseconds),
        });
        const signed = signDeliveryJob({
          secret: config.deliveryJobSigningSecret,
          payload,
        });
        yield* sender.send(signed).pipe(
          Effect.mapError(
            (cause) =>
              new DeliveryQueueSendError({
                operation: "send",
                jobId,
                kind: "push_notification",
                userId: input.userId,
                deviceId: input.deviceId,
                cause,
              }),
          ),
        );
        return {
          deviceId: input.deviceId,
          kind: "push_notification" as const,
          ok: true,
          queued: true,
          deliveryStatus: null,
          deliveryReason: null,
          providerMessageId: null,
        };
      },
    ),
  });
});

export const layer = Layer.effect(DeliveryQueue, make);

export const layerCloudflareQueues = (
  sender: Cloudflare.Queues.WriteQueueClient,
  alchemyRuntimeContext: Alchemy.BaseRuntimeContext,
) =>
  layer.pipe(
    Layer.provide(
      Layer.succeed(
        DeliveryQueueSender,
        DeliveryQueueSender.of({
          send: (body) =>
            sender
              .send(body)
              .pipe(Effect.provideService(Alchemy.RuntimeContext, alchemyRuntimeContext)),
        }),
      ),
    ),
  );
