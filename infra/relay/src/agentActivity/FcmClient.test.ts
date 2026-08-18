import { describe, expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Redacted from "effect/Redacted";
import * as Schema from "effect/Schema";
import * as HttpClient from "effect/unstable/http/HttpClient";
import * as HttpClientError from "effect/unstable/http/HttpClientError";
import * as HttpClientResponse from "effect/unstable/http/HttpClientResponse";

import type { FcmCredentials } from "../Config.ts";
import * as FcmClient from "./FcmClient.ts";
import * as FcmProviderTokens from "./FcmProviderTokens.ts";

const credentials: FcmCredentials = {
  projectId: "test-project",
  clientEmail: "firebase-adminsdk@test.iam.gserviceaccount.com",
  privateKey: Redacted.make("not-a-private-key"),
  tokenUri: "https://oauth2.googleapis.com/token",
};

const stubProviderTokens = Layer.succeed(FcmProviderTokens.FcmProviderTokens, {
  getAccessToken: () => Effect.succeed("stub-access-token"),
});

const deadHttpClient = Layer.succeed(
  HttpClient.HttpClient,
  HttpClient.make(() => Effect.die("unexpected FCM HTTP request")),
);

function webResponse(request: unknown, body: string, status: number) {
  return HttpClientResponse.fromWeb(request as never, new Response(body, { status }));
}

describe("FcmClient", () => {
  it.effect("builds a push-notification request from a notification payload", () =>
    Effect.gen(function* () {
      const fcm = yield* FcmClient.FcmClient;
      const request = fcm.makePushNotificationRequest({
        token: "fcm-token",
        notification: {
          title: "Thread",
          body: "Input: Project",
          environmentId: "env",
          threadId: "thread",
          deepLink: "/threads/env/thread",
        },
      });

      expect(request.token).toBe("fcm-token");
      expect(request.payload).toMatchObject({
        message: {
          token: "fcm-token",
          notification: { title: "Thread", body: "Input: Project" },
          data: {
            environmentId: "env",
            threadId: "thread",
            deepLink: "/threads/env/thread",
          },
        },
      });
    }).pipe(
      Effect.provide(
        FcmClient.layer.pipe(Layer.provide(stubProviderTokens), Layer.provide(deadHttpClient)),
      ),
    ),
  );

  it.effect("builds a compact Android Live Update data message", () =>
    Effect.gen(function* () {
      const fcm = yield* FcmClient.FcmClient;
      const payload = yield* fcm.encodeAndroidLiveUpdatePayload({
        generationId: "generation-1",
        eventAt: "2026-08-15T00:00:01.000Z",
        aggregate: {
          title: "S5 Code",
          subtitle: "Agent work in progress",
          activeCount: 1,
          updatedAt: "2026-08-15T00:00:00.000Z",
          activities: [
            {
              environmentId: "env" as never,
              threadId: "thread" as never,
              projectTitle: "Project",
              threadTitle: "Thread",
              modelTitle: "gpt-5.4",
              phase: "running",
              status: "Working",
              updatedAt: "2026-08-15T00:00:00.000Z",
              deepLink: "/threads/env/thread",
              planProgress: { step: "Build APK", completedSteps: 2, totalSteps: 4 },
            },
          ],
        },
      });
      const request = fcm.makeAndroidLiveUpdateRequest({ token: "fcm-token", payload });

      expect(request.payload).toMatchObject({
        message: {
          token: "fcm-token",
          data: { t3Type: "android_live_update" },
          android: { priority: "HIGH", ttl: "600s" },
        },
      });
      expect(
        (request.payload as { message: { android: unknown } }).message.android,
      ).not.toHaveProperty("collapseKey");
      const payloadText = yield* Schema.encodeEffect(Schema.fromJsonString(Schema.Unknown))(
        request.payload,
      );
      expect(payloadText).not.toContain('"notification"');
      expect(payloadText).not.toContain('"collapseKey"');
      expect(payloadText.length).toBeLessThan(4096);
    }).pipe(
      Effect.provide(
        FcmClient.layer.pipe(Layer.provide(stubProviderTokens), Layer.provide(deadHttpClient)),
      ),
    ),
  );

  it.effect("parses the message id from a successful send response", () => {
    const httpClient = HttpClient.make((request) =>
      Effect.succeed(
        webResponse(
          request,
          JSON.stringify({ name: "projects/test-project/messages/msg-123" }),
          200,
        ),
      ),
    );
    const layer = FcmClient.layer.pipe(
      Layer.provide(stubProviderTokens),
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const fcm = yield* FcmClient.FcmClient;
      const request = fcm.makePushNotificationRequest({
        token: "fcm-token",
        notification: {
          title: "Thread",
          body: "Input: Project",
          environmentId: "env",
          threadId: "thread",
          deepLink: "/threads/env/thread",
        },
      });
      const result = yield* fcm.sendPushNotificationRequest({
        credentials,
        request,
        issuedAtUnixSeconds: 0,
      });

      expect(result).toEqual({ ok: true, status: 200, messageId: "msg-123" });
    }).pipe(Effect.provide(layer));
  });

  it.effect("surfaces the FCM error code on a failed send", () => {
    const httpClient = HttpClient.make((request) =>
      Effect.succeed(
        webResponse(
          request,
          JSON.stringify({
            error: {
              status: "NOT_FOUND",
              message: "Requested entity was not found.",
              details: [
                {
                  "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError",
                  errorCode: "UNREGISTERED",
                },
              ],
            },
          }),
          404,
        ),
      ),
    );
    const layer = FcmClient.layer.pipe(
      Layer.provide(stubProviderTokens),
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const fcm = yield* FcmClient.FcmClient;
      const request = fcm.makePushNotificationRequest({
        token: "fcm-token",
        notification: {
          title: "Thread",
          body: "Input: Project",
          environmentId: "env",
          threadId: "thread",
          deepLink: "/threads/env/thread",
        },
      });
      const result = yield* fcm.sendPushNotificationRequest({
        credentials,
        request,
        issuedAtUnixSeconds: 0,
      });

      expect(result).toEqual({
        ok: false,
        status: 404,
        reason: "UNREGISTERED",
        messageId: null,
      });
    }).pipe(Effect.provide(layer));
  });

  it.effect("preserves request context and the transport cause on network failure", () => {
    const httpCause = new Error("network unavailable");
    const failingHttpClient = HttpClient.make((request) =>
      Effect.fail(
        new HttpClientError.HttpClientError({
          reason: new HttpClientError.TransportError({ request, cause: httpCause }),
        }),
      ),
    );
    const layer = FcmClient.layer.pipe(
      Layer.provide(stubProviderTokens),
      Layer.provide(Layer.succeed(HttpClient.HttpClient, failingHttpClient)),
    );
    const isFcmHttpRequestError = (value: unknown): value is FcmClient.FcmHttpRequestError =>
      value instanceof Object && (value as { _tag?: string })._tag === "FcmHttpRequestError";

    return Effect.gen(function* () {
      const fcm = yield* FcmClient.FcmClient;
      const request = fcm.makePushNotificationRequest({
        token: "fcm-token",
        notification: {
          title: "Thread",
          body: "Input: Project",
          environmentId: "env",
          threadId: "thread",
          deepLink: "/threads/env/thread",
        },
      });
      const error = yield* Effect.flip(
        fcm.sendPushNotificationRequest({ credentials, request, issuedAtUnixSeconds: 0 }),
      );

      expect(isFcmHttpRequestError(error)).toBe(true);
      if (!isFcmHttpRequestError(error)) {
        return yield* Effect.die("expected FCM HTTP request error");
      }
      expect(error).toMatchObject({
        requestKind: "push-notification",
        projectId: "test-project",
        stage: "send",
        status: null,
      });
      expect(error.cause).toBeInstanceOf(HttpClientError.HttpClientError);
    }).pipe(Effect.provide(layer));
  });
});
