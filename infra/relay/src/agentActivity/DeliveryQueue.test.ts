import * as NodeCryptoLayer from "@effect/platform-node/NodeCrypto";
import { describe, expect, it } from "@effect/vitest";
import * as Alchemy from "alchemy";
import * as Cloudflare from "alchemy/Cloudflare";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Redacted from "effect/Redacted";

import * as RelayConfiguration from "../Config.ts";
import * as DeliveryQueue from "./DeliveryQueue.ts";

const config: RelayConfiguration.RelayConfiguration["Service"] = {
  relayIssuer: "https://relay.example.com",
  apns: {
    teamId: "team-1",
    keyId: "key-1",
    privateKey: Redacted.make("apns-private-key"),
    bundleId: "com.t3tools.test",
    environment: "sandbox",
  },
  fcm: {
    projectId: "test-project",
    clientEmail: "firebase-adminsdk@test.iam.gserviceaccount.com",
    privateKey: Redacted.make("not-a-private-key"),
    tokenUri: "https://oauth2.googleapis.com/token",
  },
  clerkSecretKey: Redacted.make("clerk-secret"),
  clerkPublishableKey: "pk_test_test",
  clerkJwtAudience: "t3-code-relay",
  deliveryJobSigningSecret: Redacted.make("apns-job-secret"),
  cloudMintPrivateKey: Redacted.make("cloud-private-key"),
  cloudMintPublicKey: "cloud-public-key",
  managedEndpointBaseDomain: undefined,
  managedEndpointNamespace: undefined,
};

describe("DeliveryQueue", () => {
  it.effect("does not require the deployment RuntimeContext when building the Worker layer", () => {
    const sent: unknown[] = [];
    const sender: Cloudflare.Queues.WriteQueueClient = {
      raw: Effect.die("raw queue binding is not used"),
      send: (body) =>
        Effect.sync(() => {
          sent.push(body);
        }),
      sendBatch: () => Effect.die("batch queue binding is not used"),
    };
    const runtimeContext = {} as Alchemy.BaseRuntimeContext;
    const layer = DeliveryQueue.layerCloudflareQueues(sender, runtimeContext).pipe(
      Layer.provide(NodeCryptoLayer.layer),
      Layer.provide(RelayConfiguration.layer(config)),
    );

    return Effect.gen(function* () {
      const queue = yield* DeliveryQueue.DeliveryQueue;
      yield* queue.enqueuePushNotification({
        userId: "user-1",
        deviceId: "device-1",
        channel: "apns",
        token: "push-token",
        notification: {
          title: "Thread",
          body: "Input: Project",
          environmentId: "env-1",
          threadId: "thread-1",
          deepLink: "/threads/env-1/thread-1",
        },
      });

      expect(sent).toHaveLength(1);
    }).pipe(Effect.provide(layer));
  });

  it.effect("preserves job identity and the queue sender cause", () => {
    const cause = new Error("queue unavailable");
    const senderCause = new Cloudflare.Queues.SendError({
      message: cause.message,
      cause,
    });
    const layer = DeliveryQueue.layer.pipe(
      Layer.provide(NodeCryptoLayer.layer),
      Layer.provide(RelayConfiguration.layer(config)),
      Layer.provide(
        Layer.succeed(DeliveryQueue.DeliveryQueueSender, {
          send: () => Effect.fail(senderCause),
        }),
      ),
    );

    return Effect.gen(function* () {
      const queue = yield* DeliveryQueue.DeliveryQueue;
      const error = yield* Effect.flip(
        queue.enqueuePushNotification({
          userId: "user-1",
          deviceId: "device-1",
          channel: "apns",
          token: "push-token",
          notification: {
            title: "Thread",
            body: "Input: Project",
            environmentId: "env-1",
            threadId: "thread-1",
            deepLink: "/threads/env-1/thread-1",
          },
        }),
      );

      expect(error).toMatchObject({
        _tag: "DeliveryQueueSendError",
        operation: "send",
        jobId: expect.any(String),
        kind: "push_notification",
        userId: "user-1",
        deviceId: "device-1",
        cause: senderCause,
      });
      expect(senderCause.cause).toBe(cause);
      expect(error.message).toBe(
        "Failed to enqueue push notification delivery during send for device device-1.",
      );
    }).pipe(Effect.provide(layer));
  });
});
