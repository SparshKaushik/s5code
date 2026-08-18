import * as NodeCrypto from "node:crypto";

import * as Effect from "effect/Effect";
import * as Encoding from "effect/Encoding";
import * as Redacted from "effect/Redacted";
import * as Schema from "effect/Schema";

import type { FcmCredentials } from "../Config.ts";

export class FcmJwtEncodingError extends Schema.TaggedErrorClass<FcmJwtEncodingError>()(
  "FcmJwtEncodingError",
  {
    component: Schema.Literals(["header", "payload"]),
    clientEmail: Schema.String,
    issuedAtUnixSeconds: Schema.Number,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to encode FCM access-token JWT ${this.component} for ${this.clientEmail}.`;
  }
}

export class FcmJwtSigningError extends Schema.TaggedErrorClass<FcmJwtSigningError>()(
  "FcmJwtSigningError",
  {
    clientEmail: Schema.String,
    issuedAtUnixSeconds: Schema.Number,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to sign FCM access-token JWT for ${this.clientEmail}.`;
  }
}

export type FcmJwtError = FcmJwtEncodingError | FcmJwtSigningError;

export const FCM_ACCESS_TOKEN_SCOPE = "https://www.googleapis.com/auth/firebase.messaging";

const encodeFcmJwtHeaderJson = Schema.encodeEffect(
  Schema.fromJsonString(
    Schema.Struct({
      alg: Schema.Literal("RS256"),
      typ: Schema.Literal("JWT"),
    }),
  ),
);
const encodeFcmJwtPayloadJson = Schema.encodeEffect(
  Schema.fromJsonString(
    Schema.Struct({
      iss: Schema.String,
      scope: Schema.String,
      aud: Schema.String,
      iat: Schema.Number,
      exp: Schema.Number,
    }),
  ),
);

export interface FcmJwtSigningInput {
  readonly clientEmail: FcmCredentials["clientEmail"];
  readonly privateKey: FcmCredentials["privateKey"];
  readonly tokenUri: FcmCredentials["tokenUri"];
  readonly issuedAtUnixSeconds: number;
  readonly expiresAtUnixSeconds: number;
}

// A Google OAuth2 service-account assertion: an RS256-signed JWT whose payload
// carries the client email (iss), the Firebase Messaging scope, and the token
// endpoint as audience. Unlike APNs' deterministic ES256 provider token, this
// JWT is only the *input* to Google's token endpoint, which mints the actual
// bearer access token used on FCM send requests.
export const makeFcmAccessTokenJwt = Effect.fn("relay.fcm.make_access_token_jwt")(function* (
  input: FcmJwtSigningInput,
) {
  const headerJson = yield* encodeFcmJwtHeaderJson({ alg: "RS256", typ: "JWT" }).pipe(
    Effect.mapError(
      (cause) =>
        new FcmJwtEncodingError({
          component: "header",
          clientEmail: input.clientEmail,
          issuedAtUnixSeconds: input.issuedAtUnixSeconds,
          cause,
        }),
    ),
  );
  const payloadJson = yield* encodeFcmJwtPayloadJson({
    iss: input.clientEmail,
    scope: FCM_ACCESS_TOKEN_SCOPE,
    aud: input.tokenUri,
    iat: input.issuedAtUnixSeconds,
    exp: input.expiresAtUnixSeconds,
  }).pipe(
    Effect.mapError(
      (cause) =>
        new FcmJwtEncodingError({
          component: "payload",
          clientEmail: input.clientEmail,
          issuedAtUnixSeconds: input.issuedAtUnixSeconds,
          cause,
        }),
    ),
  );

  // Secret managers and environment variables commonly preserve PEM line
  // breaks as the two characters `\\n`. Node's key parser expects actual
  // newlines, so normalize both representations before signing.
  const privateKey = Redacted.value(input.privateKey).replace(/\\n/gu, "\n");
  const header = Encoding.encodeBase64Url(headerJson);
  const payload = Encoding.encodeBase64Url(payloadJson);
  const signingInput = `${header}.${payload}`;

  return yield* Effect.try({
    try: () => {
      const signature = NodeCrypto.createSign("RSA-SHA256")
        .update(signingInput)
        .sign(privateKey, "base64url");
      return `${signingInput}.${signature}`;
    },
    catch: (cause) =>
      new FcmJwtSigningError({
        clientEmail: input.clientEmail,
        issuedAtUnixSeconds: input.issuedAtUnixSeconds,
        cause,
      }),
  });
});
