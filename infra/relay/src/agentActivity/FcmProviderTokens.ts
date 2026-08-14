import * as NodeCrypto from "node:crypto";

import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Redacted from "effect/Redacted";
import * as Schema from "effect/Schema";
import * as HttpClient from "effect/unstable/http/HttpClient";
import * as HttpClientRequest from "effect/unstable/http/HttpClientRequest";
import * as HttpClientResponse from "effect/unstable/http/HttpClientResponse";

import type { FcmCredentials } from "../Config.ts";
import { makeFcmAccessTokenJwt, type FcmJwtError } from "./fcmJwt.ts";

export class FcmAccessTokenRequestError extends Schema.TaggedErrorClass<FcmAccessTokenRequestError>()(
  "FcmAccessTokenRequestError",
  {
    stage: Schema.Literals(["send", "exchange", "decode"]),
    status: Schema.NullOr(Schema.Number),
    projectId: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `FCM access-token request failed during ${this.stage} for project ${this.projectId}.`;
  }
}

export type FcmAccessTokenError = FcmJwtError | FcmAccessTokenRequestError;

const FcmAccessTokenResponse = Schema.Struct({
  access_token: Schema.String,
  expires_in: Schema.Number,
  token_type: Schema.String,
});

// Google access tokens live 60 minutes. Reuse each token for most of its
// lifetime so a burst of pushes does not hammer the token endpoint.
const FCM_ACCESS_TOKEN_LIFETIME_SECONDS = 60 * 60;
const FCM_ACCESS_TOKEN_REUSE_SECONDS = 45 * 60;

export class FcmProviderTokens extends Context.Service<
  FcmProviderTokens,
  {
    readonly getAccessToken: (input: {
      readonly credentials: FcmCredentials;
      readonly issuedAtUnixSeconds: number;
    }) => Effect.Effect<string, FcmAccessTokenError>;
  }
>()("t3code-relay/agentActivity/FcmProviderTokens") {}

interface CachedAccessToken {
  readonly accessToken: string;
  readonly expiresAtUnixSeconds: number;
}

// Per-isolate cache: every worker isolate mints its own access token (Google
// tolerates concurrent tokens, unlike APNs' provider-token rate limit). The
// map only avoids re-fetching on every push within one isolate.
const isolateTokenCache = new Map<string, CachedAccessToken>();

export function __resetFcmProviderTokenCacheForTest(): void {
  isolateTokenCache.clear();
}

function fcmProviderTokenCacheKey(credentials: FcmCredentials): string {
  const keyFingerprint = NodeCrypto.createHash("sha256")
    .update(Redacted.value(credentials.privateKey))
    .digest("hex")
    .slice(0, 16);
  return `${credentials.projectId}:${credentials.clientEmail}:${keyFingerprint}`;
}

export const make = Effect.gen(function* () {
  const httpClient = yield* HttpClient.HttpClient;

  return FcmProviderTokens.of({
    getAccessToken: Effect.fn("relay.fcm.get_access_token")(function* (input) {
      yield* Effect.annotateCurrentSpan({
        "relay.fcm.project_id": input.credentials.projectId,
      });
      const now = input.issuedAtUnixSeconds;
      const cacheKey = fcmProviderTokenCacheKey(input.credentials);
      const cached = isolateTokenCache.get(cacheKey);
      if (cached && cached.expiresAtUnixSeconds - now > FCM_ACCESS_TOKEN_REUSE_SECONDS) {
        return cached.accessToken;
      }

      const jwt = yield* makeFcmAccessTokenJwt({
        clientEmail: input.credentials.clientEmail,
        privateKey: input.credentials.privateKey,
        tokenUri: input.credentials.tokenUri,
        issuedAtUnixSeconds: now,
        expiresAtUnixSeconds: now + FCM_ACCESS_TOKEN_LIFETIME_SECONDS,
      });

      const response = yield* HttpClientRequest.post(input.credentials.tokenUri).pipe(
        HttpClientRequest.bodyUrlParams({
          grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
          assertion: jwt,
        }),
        httpClient.execute,
        Effect.mapError(
          (cause) =>
            new FcmAccessTokenRequestError({
              stage: "send",
              status: null,
              projectId: input.credentials.projectId,
              cause,
            }),
        ),
      );

      if (response.status < 200 || response.status >= 300) {
        const body = yield* response.text.pipe(Effect.orElseSucceed(() => ""));
        return yield* new FcmAccessTokenRequestError({
          stage: "exchange",
          status: response.status,
          projectId: input.credentials.projectId,
          cause: new Error(`FCM token endpoint returned status ${response.status}: ${body}`),
        });
      }

      const token = yield* HttpClientResponse.schemaBodyJson(FcmAccessTokenResponse)(response).pipe(
        Effect.mapError(
          (cause) =>
            new FcmAccessTokenRequestError({
              stage: "decode",
              status: response.status,
              projectId: input.credentials.projectId,
              cause,
            }),
        ),
      );

      const expiresAtUnixSeconds = now + token.expires_in;
      isolateTokenCache.set(cacheKey, {
        accessToken: token.access_token,
        expiresAtUnixSeconds,
      });
      return token.access_token;
    }),
  });
});

export const layer = Layer.effect(FcmProviderTokens, make);
