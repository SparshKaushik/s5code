import { describe, expect, it } from "@effect/vitest";
import * as NodeCrypto from "node:crypto";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Redacted from "effect/Redacted";
import * as HttpClient from "effect/unstable/http/HttpClient";
import * as HttpClientResponse from "effect/unstable/http/HttpClientResponse";

import type { FcmCredentials } from "../Config.ts";
import * as FcmProviderTokens from "./FcmProviderTokens.ts";

const { privateKey: fcmPrivateKey } = NodeCrypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
  publicKeyEncoding: { type: "spki", format: "pem" },
});

const credentials: FcmCredentials = {
  projectId: "test-project",
  clientEmail: "firebase-adminsdk@test.iam.gserviceaccount.com",
  privateKey: Redacted.make(fcmPrivateKey),
  tokenUri: "https://oauth2.googleapis.com/token",
};

function jsonResponse(request: unknown, body: unknown, status = 200) {
  return HttpClientResponse.fromWeb(
    request as never,
    new Response(JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

describe("FcmProviderTokens", () => {
  it.effect("exchanges the signed assertion for an access token", () => {
    FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    let requestCount = 0;
    const httpClient = HttpClient.make((request) => {
      requestCount += 1;
      return Effect.succeed(
        jsonResponse(request, {
          access_token: "access-token-1",
          expires_in: 3600,
          token_type: "Bearer",
        }),
      );
    });
    const layer = FcmProviderTokens.layer.pipe(
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const tokens = yield* FcmProviderTokens.FcmProviderTokens;
      const token = yield* tokens.getAccessToken({ credentials, issuedAtUnixSeconds: 0 });
      expect(token).toBe("access-token-1");
      expect(requestCount).toBe(1);
      FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    }).pipe(Effect.provide(layer));
  });

  it.effect("accepts service-account PEM keys with escaped line breaks", () => {
    FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    const httpClient = HttpClient.make((request) =>
      Effect.succeed(
        jsonResponse(request, {
          access_token: "access-token-escaped-key",
          expires_in: 3600,
          token_type: "Bearer",
        }),
      ),
    );
    const escapedCredentials: FcmCredentials = {
      ...credentials,
      privateKey: Redacted.make(fcmPrivateKey.replace(/\n/gu, "\\n")),
    };
    const layer = FcmProviderTokens.layer.pipe(
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const tokens = yield* FcmProviderTokens.FcmProviderTokens;
      const token = yield* tokens.getAccessToken({
        credentials: escapedCredentials,
        issuedAtUnixSeconds: 0,
      });
      expect(token).toBe("access-token-escaped-key");
      FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    }).pipe(Effect.provide(layer));
  });

  it.effect("reuses a cached access token within the isolate", () => {
    FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    let requestCount = 0;
    const httpClient = HttpClient.make((request) => {
      requestCount += 1;
      return Effect.succeed(
        jsonResponse(request, {
          access_token: "access-token-1",
          expires_in: 3600,
          token_type: "Bearer",
        }),
      );
    });
    const layer = FcmProviderTokens.layer.pipe(
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const tokens = yield* FcmProviderTokens.FcmProviderTokens;
      const first = yield* tokens.getAccessToken({ credentials, issuedAtUnixSeconds: 0 });
      const second = yield* tokens.getAccessToken({ credentials, issuedAtUnixSeconds: 60 });
      expect(second).toBe(first);
      expect(requestCount).toBe(1);
      FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    }).pipe(Effect.provide(layer));
  });

  it.effect("refreshes the token once the reuse window has elapsed", () => {
    FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    let requestCount = 0;
    const httpClient = HttpClient.make((request) => {
      requestCount += 1;
      return Effect.succeed(
        jsonResponse(request, {
          access_token: `access-token-${requestCount}`,
          expires_in: 3600,
          token_type: "Bearer",
        }),
      );
    });
    const layer = FcmProviderTokens.layer.pipe(
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const tokens = yield* FcmProviderTokens.FcmProviderTokens;
      const first = yield* tokens.getAccessToken({ credentials, issuedAtUnixSeconds: 0 });
      // Past the 45-minute reuse window but still within the 60-minute token
      // lifetime: the cached token is stale enough to refresh proactively.
      const second = yield* tokens.getAccessToken({
        credentials,
        issuedAtUnixSeconds: 46 * 60,
      });
      expect(second).not.toBe(first);
      expect(requestCount).toBe(2);
      FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    }).pipe(Effect.provide(layer));
  });

  it.effect("surfaces a non-2xx token endpoint response as an exchange error", () => {
    FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    const httpClient = HttpClient.make((request) =>
      Effect.succeed(jsonResponse(request, { error: "invalid_grant" }, 400)),
    );
    const layer = FcmProviderTokens.layer.pipe(
      Layer.provide(Layer.succeed(HttpClient.HttpClient, httpClient)),
    );

    return Effect.gen(function* () {
      const tokens = yield* FcmProviderTokens.FcmProviderTokens;
      const error = yield* Effect.flip(
        tokens.getAccessToken({ credentials, issuedAtUnixSeconds: 0 }),
      );
      expect(error).toMatchObject({
        _tag: "FcmAccessTokenRequestError",
        stage: "exchange",
        status: 400,
        projectId: "test-project",
      });
      FcmProviderTokens.__resetFcmProviderTokenCacheForTest();
    }).pipe(Effect.provide(layer));
  });
});
