import * as Alchemy from "alchemy";
import * as Cloudflare from "alchemy/Cloudflare";
import * as Drizzle from "alchemy/Drizzle";
import * as Config from "effect/Config";
import * as DateTime from "effect/DateTime";
import * as Crypto from "effect/Crypto";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Redacted from "effect/Redacted";
import * as Stream from "effect/Stream";
import * as Etag from "effect/unstable/http/Etag";
import * as HttpPlatform from "effect/unstable/http/HttpPlatform";
import * as HttpRouter from "effect/unstable/http/HttpRouter";
import * as HttpApiBuilder from "effect/unstable/httpapi/HttpApiBuilder";
import * as HttpApiScalar from "effect/unstable/httpapi/HttpApiScalar";

import { RelayApi } from "@t3tools/contracts/relay";

import {
  clientApi,
  dpopClientApi,
  healthApi,
  metadataApi,
  mobileApi,
  relayClientAuthLayer,
  relayDpopClientAuthLayer,
  relayCors,
  relayDocsRedirectRoute,
  relayEnvironmentAuthLayer,
  relayNotFoundRoute,
  serverApi,
  traceRelayHttpRequestWith,
  tokenApi,
  withoutCapturedParentSpan,
} from "./http/Api.ts";
import { ManagedEndpointZone, RelayApiZone, RelayDeploymentConfig } from "./zone.ts";
import { makeRelayTraceLayer, RelayObservability } from "./observability.ts";
import * as DeliveryAttempts from "./agentActivity/DeliveryAttempts.ts";
import * as AgentActivityRows from "./agentActivity/AgentActivityRows.ts";
import * as Devices from "./agentActivity/Devices.ts";
import * as DpopProofs from "./auth/DpopProofs.ts";
import * as RelayTokens from "./auth/RelayTokens.ts";
import * as EnvironmentCredentials from "./environments/EnvironmentCredentials.ts";
import * as EnvironmentLinks from "./environments/EnvironmentLinks.ts";
import * as ManagedEndpointAllocations from "./environments/ManagedEndpointAllocations.ts";
import * as LiveActivities from "./agentActivity/LiveActivities.ts";
import * as RelayDb from "./db.ts";
import { RelayDeliveryDeadLetterQueue, RelayDeliveryQueue } from "./queues.ts";
import * as RelayConfiguration from "./Config.ts";
import * as AgentActivityPublisher from "./agentActivity/AgentActivityPublisher.ts";
import * as ApnsClient from "./agentActivity/ApnsClient.ts";
import * as ApnsProviderTokens from "./agentActivity/ApnsProviderTokens.ts";
import * as FcmClient from "./agentActivity/FcmClient.ts";
import * as FcmProviderTokens from "./agentActivity/FcmProviderTokens.ts";
import * as DeliveryQueue from "./agentActivity/DeliveryQueue.ts";
import * as Deliveries from "./agentActivity/Deliveries.ts";
import * as EnvironmentConnector from "./environments/EnvironmentConnector.ts";
import * as EnvironmentLinker from "./environments/EnvironmentLinker.ts";
import * as EnvironmentPublishSignatures from "./environments/EnvironmentPublishSignatures.ts";
import * as ManagedEndpointProvider from "./environments/ManagedEndpointProvider.ts";
import * as ManagedTunnelLimits from "./environments/ManagedTunnelLimits.ts";
import * as MobileRegistrations from "./agentActivity/MobileRegistrations.ts";

const webcryptoLayer = Layer.succeed(
  Crypto.Crypto,
  Crypto.make({
    randomBytes: (size) => globalThis.crypto.getRandomValues(new Uint8Array(size)),
    digest: (algorithm, data) =>
      Effect.promise(async () => {
        const input = new Uint8Array(data.length);
        input.set(data);
        return new Uint8Array(await globalThis.crypto.subtle.digest(algorithm, input.buffer));
      }),
  }),
);

const httpPlatformNotSupportedLayer = Layer.succeed(HttpPlatform.HttpPlatform, {
  platform: "web",
  compression: {
    algorithms: new Set<HttpPlatform.CompressionAlgorithm>(),
    compressResponse: (response) => Effect.succeed(response),
  },
  fileResponse: () => Effect.die("Relay API does not serve filesystem responses"),
  fileWebResponse: () => Effect.die("Relay API does not serve file responses"),
});

const relayApiLayer = Layer.mergeAll(
  healthApi,
  metadataApi,
  mobileApi,
  clientApi,
  tokenApi,
  dpopClientApi,
  serverApi,
);

const CloudMintKeyPair = Alchemy.KeyPair("CloudMintKeyPair");
const DeliveryJobSigningSecret = Alchemy.makeRandom("DeliveryJobSigningSecret", {
  bytes: 32,
});

export class Api extends Cloudflare.Worker<Api, {}>()("Api") {}

export const ApiLive = Api.make(
  RelayDeploymentConfig.pipe(
    Effect.map(({ relayPublicDomain }) => ({
      main: import.meta.filename,
      compatibility: {
        date: "2026-05-22",
        flags: ["nodejs_compat"],
      },
      domain: relayPublicDomain,
    })),
    Effect.orDie,
  ),
  Effect.gen(function* () {
    //
    // 1. Provision Infrastructure for the Worker to use
    //
    const { relayPublicOrigin, stage } = yield* RelayDeploymentConfig;
    const deliveryQueue = yield* RelayDeliveryQueue;
    const deliveryDeadLetterQueue = yield* RelayDeliveryDeadLetterQueue;
    const cloudMintKeyPair = yield* CloudMintKeyPair;
    const relayApiZone = yield* RelayApiZone;
    const managedEndpointZone = yield* ManagedEndpointZone;
    const randomDeliveryJobSigningSecret = yield* DeliveryJobSigningSecret;
    const observability = yield* RelayObservability;

    //
    // 2. Create bindings
    //
    // APNs is optional. When any required Apple/APNs key is absent the relay
    // still deploys and serves remote connections; mobile push notifications
    // and Live Activities are simply disabled. The APNs-only layers and the
    // delivery-queue consumption below are gated on this flag, and the
    // RelayConfiguration receives placeholder credentials that are never read
    // because the delivery path is not built.
    const apnsEnvironment = yield* Config.schema(
      RelayConfiguration.ApnsEnvironment,
      "APNS_ENVIRONMENT",
    ).pipe(Config.option);
    const apnsTeamId = yield* Config.string("APNS_TEAM_ID").pipe(Config.option);
    const apnsKeyId = yield* Config.string("APNS_KEY_ID").pipe(Config.option);
    const apnsBundleId = yield* Config.string("APNS_BUNDLE_ID").pipe(Config.option);
    const apnsPrivateKey = yield* Config.redacted("APNS_PRIVATE_KEY").pipe(Config.option);
    const apnsEnabled =
      Option.isSome(apnsEnvironment) &&
      Option.isSome(apnsTeamId) &&
      Option.isSome(apnsKeyId) &&
      Option.isSome(apnsBundleId) &&
      Option.isSome(apnsPrivateKey);
    // FCM is optional too, mirroring APNs: absent credentials disable Android
    // push notifications while the relay still deploys and serves remote
    // connections.
    const fcmProjectId = yield* Config.string("FCM_PROJECT_ID").pipe(Config.option);
    const fcmClientEmail = yield* Config.string("FCM_CLIENT_EMAIL").pipe(Config.option);
    const fcmPrivateKey = yield* Config.redacted("FCM_PRIVATE_KEY").pipe(Config.option);
    const fcmEnabled =
      Option.isSome(fcmProjectId) && Option.isSome(fcmClientEmail) && Option.isSome(fcmPrivateKey);
    const deliveryJobSigningSecret = yield* randomDeliveryJobSigningSecret;
    const deliveryQueueSender = yield* Cloudflare.Queues.WriteQueue(deliveryQueue);

    const axiomDatasetName = yield* observability.traces.name;
    const axiomIngestToken = yield* observability.workerIngestToken.token;
    const axiomTracesEndpoint = yield* observability.traces.otelTracesEndpoint;

    const clerkSecretKey = yield* Config.redacted("CLERK_SECRET_KEY");
    const clerkPublishableKey = yield* Config.string("CLERK_PUBLISHABLE_KEY");
    const clerkJwtAudience = yield* Config.string("CLERK_JWT_AUDIENCE");

    const cloudMintPrivateKey = yield* cloudMintKeyPair.privateKey;
    const cloudMintPublicKey = yield* cloudMintKeyPair.publicKey;
    const hyperdrive = yield* Cloudflare.Hyperdrive.Connect(yield* RelayDb.RelayHyperdrive);
    const db = yield* Drizzle.Postgres(hyperdrive.connectionString);

    const managedEndpointTunnelBinding = yield* Cloudflare.Tunnel.ReadWriteTunnel();
    // Keep Worker custom-domain reconciliation ordered after API zone provisioning.
    yield* yield* relayApiZone.zoneId;
    const managedEndpointDnsBinding = yield* Cloudflare.DNS.ReadWriteDns(managedEndpointZone);
    const managedEndpointZoneName = yield* managedEndpointZone.name;

    //
    // 3. Runtime layers and app construction
    //
    const alchemyRuntimeContext: Alchemy.BaseRuntimeContext = yield* Cloudflare.Worker;

    const loadSettings = Effect.gen(function* () {
      return RelayConfiguration.RelayConfiguration.of({
        relayIssuer: relayPublicOrigin,
        apns: apnsEnabled
          ? {
              environment: Option.getOrThrow(apnsEnvironment),
              teamId: Option.getOrThrow(apnsTeamId),
              keyId: Option.getOrThrow(apnsKeyId),
              bundleId: Option.getOrThrow(apnsBundleId),
              privateKey: Option.getOrThrow(apnsPrivateKey),
            }
          : {
              // Placeholder. Read only when `apnsEnabled`, which selects the
              // branch above; when disabled the APNs delivery path is not built.
              environment: "sandbox",
              teamId: "",
              keyId: "",
              bundleId: "",
              privateKey: Redacted.make(""),
            },
        fcm: fcmEnabled
          ? {
              projectId: Option.getOrThrow(fcmProjectId),
              clientEmail: Option.getOrThrow(fcmClientEmail),
              privateKey: Option.getOrThrow(fcmPrivateKey),
              tokenUri: "https://oauth2.googleapis.com/token",
            }
          : {
              // Placeholder. Read only when `fcmEnabled`; when disabled the FCM
              // delivery path is not built.
              projectId: "",
              clientEmail: "",
              privateKey: Redacted.make(""),
              tokenUri: "https://oauth2.googleapis.com/token",
            },
        deliveryJobSigningSecret: yield* deliveryJobSigningSecret,
        clerkSecretKey,
        clerkPublishableKey,
        clerkJwtAudience,
        cloudMintPrivateKey: yield* cloudMintPrivateKey,
        cloudMintPublicKey: yield* cloudMintPublicKey,
        managedEndpointBaseDomain: yield* managedEndpointZoneName,
        managedEndpointNamespace: stage,
      });
    });

    const relayTraceLayer = Layer.unwrap(
      Effect.all({
        tracesDatasetName: axiomDatasetName,
        tracesEndpoint: axiomTracesEndpoint,
        ingestToken: axiomIngestToken,
      }).pipe(Effect.map(makeRelayTraceLayer)),
    );

    const runtimeLayer = Layer.empty.pipe(
      Layer.provideMerge(MobileRegistrations.layer),
      Layer.provideMerge(AgentActivityPublisher.layer),
      Layer.provideMerge(EnvironmentConnector.layer),
      Layer.provideMerge(EnvironmentLinker.layer),
      Layer.provideMerge(EnvironmentPublishSignatures.layer),
      Layer.provideMerge(
        ManagedEndpointProvider.layerCloudflareBindings(
          managedEndpointTunnelBinding,
          managedEndpointDnsBinding,
          alchemyRuntimeContext,
        ),
      ),
      Layer.provideMerge(DpopProofs.layer),
      // APNs services must always be provided: AgentActivityPublisher depends on
      // Deliveries, and these layers build fine with the placeholder
      // credentials used when APNs is disabled. Only the delivery-queue
      // *consumer* below is gated on `apnsEnabled`.
      Layer.provideMerge(Deliveries.layer),
      Layer.provideMerge(
        Layer.mergeAll(
          ApnsClient.layer.pipe(Layer.provideMerge(ApnsProviderTokens.layer)),
          FcmClient.layer.pipe(Layer.provideMerge(FcmProviderTokens.layer)),
        ),
      ),
      Layer.provideMerge(
        DeliveryQueue.layerCloudflareQueues(deliveryQueueSender, alchemyRuntimeContext),
      ),
      Layer.provideMerge(AgentActivityRows.layer),
      Layer.provideMerge(Devices.layer),
      Layer.provideMerge(EnvironmentCredentials.layer),
      Layer.provideMerge(
        Layer.mergeAll(
          EnvironmentLinks.layer,
          ManagedEndpointAllocations.layer,
          ManagedTunnelLimits.layer,
        ),
      ),
      Layer.provideMerge(LiveActivities.layer),
      Layer.provideMerge(DeliveryAttempts.layer),
      Layer.provideMerge(RelayTokens.layer),
      Layer.provideMerge(
        RelayDb.RelayTransactions.layer.pipe(
          Layer.provideMerge(Layer.succeed(RelayDb.RelayDb, db)),
        ),
      ),
      Layer.provideMerge(Layer.effect(RelayConfiguration.RelayConfiguration, loadSettings)),
      Layer.provideMerge(webcryptoLayer),
    );

    const appLayer = relayApiLayer.pipe(
      Layer.provideMerge(relayClientAuthLayer),
      Layer.provideMerge(relayDpopClientAuthLayer),
      Layer.provideMerge(relayEnvironmentAuthLayer),
      Layer.provide(runtimeLayer),
    );

    if (!apnsEnabled && !fcmEnabled) {
      yield* Effect.logWarning(
        "Neither APNs nor FCM configured; mobile notifications and Live Activities disabled",
      );
    } else {
      yield* Cloudflare.Queues.consumeQueueMessages<unknown>(
        deliveryQueue,
        {
          batchSize: 10,
          maxRetries: 5,
          maxWaitTime: "5 seconds",
          retryDelay: "30 seconds",
          deadLetterQueue: deliveryDeadLetterQueue.queueName as unknown as string,
        },
        (stream) =>
          stream.pipe(
            Stream.withSpan("relay.delivery_queue.process_batch"),
            Stream.runForEach((message) =>
              Deliveries.Deliveries.pipe(
                Effect.flatMap((deliveries) => deliveries.processSignedJob(message.body)),
                Effect.withSpan("relay.delivery_queue.process_message"),
              ),
            ),
            Effect.provide(runtimeLayer),
          ),
      );
    }

    yield* Cloudflare.Workers.cron("*/5 * * * *", () =>
      DpopProofs.DpopProofReplay.pipe(
        Effect.flatMap((dpopProofs) => dpopProofs.pruneExpired),
        // Terminal thread rows are kept briefly so finished agents show as
        // Done/Failed in the Live Activity; sweep them once they age out.
        Effect.andThen(
          Effect.all([AgentActivityRows.AgentActivityRows, DateTime.now]).pipe(
            Effect.flatMap(([activityRows, now]) =>
              activityRows.pruneTerminal({
                updatedBefore: DateTime.formatIso(DateTime.subtract(now, { minutes: 30 })),
              }),
            ),
          ),
        ),
        Effect.withSpan("relay.cron.prune_expired_state"),
        Effect.provide(runtimeLayer),
      ),
    );

    const fetch = Layer.merge(
      Layer.mergeAll(
        HttpApiBuilder.layer(RelayApi, { openapiPath: "/openapi.json" }).pipe(
          Layer.provide(appLayer),
        ),
        HttpApiScalar.layer(RelayApi, { path: "/docs" }),
        relayDocsRedirectRoute,
      ).pipe(Layer.provide([Etag.layerWeak, httpPlatformNotSupportedLayer, relayCors])),
      relayNotFoundRoute,
    ).pipe(
      HttpRouter.toHttpEffect,
      withoutCapturedParentSpan,
      Effect.flatMap((httpEffect) => traceRelayHttpRequestWith(httpEffect, relayTraceLayer)),
    );

    return { fetch };
  }).pipe(
    Effect.provide(
      Layer.empty.pipe(
        Layer.provideMerge(Cloudflare.Hyperdrive.ConnectBinding),
        Layer.provideMerge(Cloudflare.Workers.CronEventSourceLive),
        Layer.provideMerge(Cloudflare.Queues.WriteQueueBinding),
        Layer.provideMerge(Cloudflare.Queues.EventSourceLive),
        Layer.provideMerge(Cloudflare.Tunnel.ReadWriteTunnelBinding),
        Layer.provideMerge(Cloudflare.DNS.ReadWriteDnsHttp),
      ),
    ),
  ),
);

export default ApiLive;
