import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";
import * as HttpClient from "effect/unstable/http/HttpClient";
import * as HttpClientRequest from "effect/unstable/http/HttpClientRequest";

import type { FcmCredentials } from "../Config.ts";
import {
  RelayAgentActivityAggregateState,
  type RelayAgentActivityAggregateState as RelayAgentActivityAggregateStateType,
} from "@t3tools/contracts/relay";
import type { NotificationPayload } from "./deliveryJobs.ts";
import { FcmAccessTokenRequestError, FcmProviderTokens } from "./FcmProviderTokens.ts";
import { FcmJwtEncodingError, FcmJwtSigningError } from "./fcmJwt.ts";

export { FcmJwtEncodingError, FcmJwtSigningError } from "./fcmJwt.ts";

interface FcmPushNotificationRequest {
  readonly token: string;
  readonly payload: unknown;
}

interface FcmAndroidLiveUpdateRequest {
  readonly token: string;
  readonly payload: unknown;
}

export interface FcmDeliveryResult {
  readonly ok: boolean;
  readonly status: number;
  readonly reason?: string;
  readonly messageId: string | null;
}

export class FcmHttpRequestError extends Schema.TaggedErrorClass<FcmHttpRequestError>()(
  "FcmHttpRequestError",
  {
    requestKind: Schema.Literals(["push-notification", "android-live-update"]),
    projectId: Schema.String,
    tokenSuffix: Schema.String,
    stage: Schema.Literals(["send", "read-response"]),
    status: Schema.NullOr(Schema.Number),
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `FCM ${this.requestKind} request failed during ${this.stage} for project ${this.projectId}.`;
  }
}

export const FcmError = Schema.Union([
  FcmJwtEncodingError,
  FcmJwtSigningError,
  FcmAccessTokenRequestError,
  FcmHttpRequestError,
]);
export type FcmError = typeof FcmError.Type;

const FcmSendResponse = Schema.Struct({
  name: Schema.String,
});
const decodeFcmSendResponseJson = Schema.decodeUnknownOption(
  Schema.fromJsonString(FcmSendResponse),
);

const FcmErrorResponse = Schema.Struct({
  error: Schema.Struct({
    status: Schema.optional(Schema.String),
    message: Schema.optional(Schema.String),
    details: Schema.optional(
      Schema.Array(
        Schema.Struct({
          errorCode: Schema.optional(Schema.String),
        }),
      ),
    ),
  }),
});
const decodeFcmErrorResponseJson = Schema.decodeUnknownOption(
  Schema.fromJsonString(FcmErrorResponse),
);

// The FCM v1 send endpoint. `name` in the success response is
// `projects/{projectId}/messages/{messageId}`; the trailing segment is the
// canonical message identifier we surface as the delivery's provider id.
function messageIdFromName(name: string): string | null {
  const parts = name.split("/");
  const last = parts[parts.length - 1];
  return last && last.length > 0 ? last : null;
}

function fcmReasonFromBody(body: string): string | undefined {
  if (body.trim().length === 0) {
    return undefined;
  }
  return Option.match(decodeFcmErrorResponseJson(body), {
    onNone: () => body,
    onSome: (parsed) => {
      const detailCode = parsed.error.details?.find((detail) => detail.errorCode)?.errorCode;
      return detailCode ?? parsed.error.status ?? parsed.error.message ?? body;
    },
  });
}

function makePushNotificationRequest(input: {
  readonly token: string;
  readonly notification: NotificationPayload;
}): FcmPushNotificationRequest {
  const data: Record<string, string> = {
    environmentId: input.notification.environmentId,
    threadId: input.notification.threadId,
    deepLink: input.notification.deepLink,
  };
  if (input.notification.phase !== undefined) {
    data.phase = input.notification.phase;
  }
  if (input.notification.updatedAt !== undefined) {
    data.updatedAt = input.notification.updatedAt;
  }
  return {
    token: input.token,
    payload: {
      message: {
        token: input.token,
        notification: {
          title: input.notification.title,
          body: input.notification.body,
        },
        data,
        android: {
          priority: "HIGH",
        },
      },
    },
  };
}

const FcmAndroidLiveUpdatePayload = Schema.Struct({
  type: Schema.Literal("agent_activity_live_update"),
  version: Schema.Literal(1),
  event: Schema.Literals(["update", "end"]),
  generationId: Schema.String,
  eventAt: Schema.String,
  aggregate: Schema.NullOr(RelayAgentActivityAggregateState),
});
const decodeFcmAndroidLiveUpdatePayload = Schema.decodeUnknownEffect(FcmAndroidLiveUpdatePayload);
const encodeFcmAndroidLiveUpdatePayload = Schema.encodeEffect(
  Schema.fromJsonString(FcmAndroidLiveUpdatePayload),
);

const MAX_FCM_DATA_PAYLOAD_BYTES = 4_096;

function makeAndroidLiveUpdateRequest(input: {
  readonly token: string;
  readonly payload: string;
}): FcmAndroidLiveUpdateRequest {
  if (new TextEncoder().encode(input.payload).byteLength > MAX_FCM_DATA_PAYLOAD_BYTES) {
    throw new RangeError("Android Live Update FCM payload exceeds 4096 bytes.");
  }
  return {
    token: input.token,
    payload: {
      message: {
        token: input.token,
        data: {
          t3Type: "android_live_update",
          liveUpdate: input.payload,
        },
        android: {
          priority: "HIGH",
          ttl: "600s",
          collapseKey: "agent-live-update",
        },
      },
    },
  };
}

export class FcmClient extends Context.Service<
  FcmClient,
  {
    readonly makePushNotificationRequest: typeof makePushNotificationRequest;
    readonly makeAndroidLiveUpdateRequest: typeof makeAndroidLiveUpdateRequest;
    readonly encodeAndroidLiveUpdatePayload: (input: {
      readonly generationId: string;
      readonly eventAt: string;
      readonly aggregate: RelayAgentActivityAggregateStateType | null;
    }) => Effect.Effect<string, Schema.SchemaError>;
    readonly sendPushNotificationRequest: (input: {
      readonly credentials: FcmCredentials;
      readonly request: FcmPushNotificationRequest | FcmAndroidLiveUpdateRequest;
      readonly requestKind?: "push-notification" | "android-live-update";
      readonly issuedAtUnixSeconds: number;
    }) => Effect.Effect<FcmDeliveryResult, FcmError>;
  }
>()("t3code-relay/agentActivity/FcmClient") {}

export const make = Effect.gen(function* () {
  const httpClient = yield* HttpClient.HttpClient;
  const providerTokens = yield* FcmProviderTokens;

  const sendPushNotificationRequest: FcmClient["Service"]["sendPushNotificationRequest"] =
    Effect.fn("relay.fcm.send_push_notification_request")(function* (input) {
      const requestKind = input.requestKind ?? "push-notification";
      yield* Effect.annotateCurrentSpan({ "relay.fcm.event": requestKind });
      const accessToken = yield* providerTokens.getAccessToken({
        credentials: input.credentials,
        issuedAtUnixSeconds: input.issuedAtUnixSeconds,
      });
      const host = `https://fcm.googleapis.com/v1/projects/${input.credentials.projectId}/messages:send`;
      const response = yield* HttpClientRequest.post(host).pipe(
        HttpClientRequest.setHeaders({
          authorization: `Bearer ${accessToken}`,
          "content-type": "application/json",
        }),
        HttpClientRequest.bodyJson(input.request.payload),
        Effect.flatMap(httpClient.execute),
        Effect.mapError(
          (cause) =>
            new FcmHttpRequestError({
              requestKind,
              projectId: input.credentials.projectId,
              tokenSuffix: input.request.token.slice(-8),
              stage: "send",
              status: null,
              cause,
            }),
        ),
      );
      const responseText = yield* response.text.pipe(
        Effect.mapError(
          (cause) =>
            new FcmHttpRequestError({
              requestKind,
              projectId: input.credentials.projectId,
              tokenSuffix: input.request.token.slice(-8),
              stage: "read-response",
              status: response.status,
              cause,
            }),
        ),
      );
      const ok = response.status >= 200 && response.status < 300;
      const reason = ok ? undefined : fcmReasonFromBody(responseText);
      const messageId = Option.match(decodeFcmSendResponseJson(responseText), {
        onNone: () => null,
        onSome: (parsed) => messageIdFromName(parsed.name),
      });
      return {
        ok,
        status: response.status,
        ...(reason === undefined ? {} : { reason }),
        messageId,
      };
    });

  return FcmClient.of({
    makePushNotificationRequest,
    makeAndroidLiveUpdateRequest,
    encodeAndroidLiveUpdatePayload: (input) =>
      decodeFcmAndroidLiveUpdatePayload({
        type: "agent_activity_live_update",
        version: 1,
        event: input.aggregate === null ? "end" : "update",
        generationId: input.generationId,
        eventAt: input.eventAt,
        aggregate: input.aggregate,
      }).pipe(Effect.flatMap(encodeFcmAndroidLiveUpdatePayload)),
    sendPushNotificationRequest,
  });
});

export const layer = Layer.effect(FcmClient, make);
