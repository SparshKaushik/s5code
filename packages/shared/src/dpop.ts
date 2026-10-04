import { p256 } from "@noble/curves/nist";
import { sha256 } from "@noble/hashes/sha2";
import * as Encoding from "effect/Encoding";
import * as Option from "effect/Option";
import * as Result from "effect/Result";
import * as Schema from "effect/Schema";

import { DpopPublicJwk as DpopPublicJwkSchema, normalizeDpopHtu } from "./dpopCommon.ts";
import type { DpopPublicJwk as DpopPublicJwkType } from "./dpopCommon.ts";
import { stableStringify } from "./relaySigning.ts";

const DPOP_TYP = "dpop+jwt";
const DPOP_ALG = "ES256";
const DEFAULT_MAX_AGE_SECONDS = 300;

export const DpopPublicJwk = DpopPublicJwkSchema;
export type DpopPublicJwk = DpopPublicJwkType;
export { normalizeDpopHtu };

export const DpopVerificationFailureCode = Schema.Literals([
  "missing_proof",
  "malformed_proof",
  "key_mismatch",
  "method_mismatch",
  "url_mismatch",
  "access_token_hash_mismatch",
  "time_window",
  "invalid_signature",
  "invalid_proof",
]);
export type DpopVerificationFailureCode = typeof DpopVerificationFailureCode.Type;

const DpopJwtHeaderPublicJwk = Schema.Struct({
  ...DpopPublicJwkSchema.fields,
  d: Schema.optionalKey(Schema.Never),
});

const DpopJwtHeaderJson = Schema.fromJsonString(
  Schema.Struct({
    typ: Schema.Literal(DPOP_TYP),
    alg: Schema.Literal(DPOP_ALG),
    jwk: DpopJwtHeaderPublicJwk,
  }),
);
const decodeDpopJwtHeaderJson = Schema.decodeUnknownOption(DpopJwtHeaderJson);

const DpopJwtPayloadJson = Schema.fromJsonString(
  Schema.Struct({
    htm: Schema.String.check(Schema.isNonEmpty()),
    htu: Schema.String.check(Schema.isNonEmpty()),
    jti: Schema.String.check(Schema.isNonEmpty()),
    iat: Schema.Int,
    ath: Schema.optionalKey(Schema.String),
  }),
);
const decodeDpopJwtPayloadJson = Schema.decodeUnknownOption(DpopJwtPayloadJson);

export type DpopVerificationResult =
  | {
      readonly ok: true;
      readonly thumbprint: string;
      readonly jti: string;
      readonly iat: number;
    }
  | {
      readonly ok: false;
      readonly code: DpopVerificationFailureCode;
      readonly reason: string;
    };

function base64UrlToBytes(value: string): Uint8Array {
  return Result.getOrThrow(Encoding.decodeBase64Url(value));
}

function decodeBase64UrlDpopJwtHeader(value: string) {
  return decodeDpopJwtHeaderJson(Result.getOrThrow(Encoding.decodeBase64UrlString(value)));
}

function decodeBase64UrlDpopJwtPayload(value: string) {
  return decodeDpopJwtPayloadJson(Result.getOrThrow(Encoding.decodeBase64UrlString(value)));
}

function dpopThumbprintInput(jwk: DpopPublicJwkType): string {
  return stableStringify({
    crv: jwk.crv,
    kty: jwk.kty,
    x: jwk.x,
    y: jwk.y,
  });
}

export function computeDpopJwkThumbprint(jwk: DpopPublicJwkType): string {
  return Encoding.encodeBase64Url(sha256(new TextEncoder().encode(dpopThumbprintInput(jwk))));
}

export function computeDpopAccessTokenHash(accessToken: string): string {
  return Encoding.encodeBase64Url(sha256(new TextEncoder().encode(accessToken)));
}

function publicKeyBytesFromJwk(jwk: DpopPublicJwkType): Uint8Array {
  const x = base64UrlToBytes(jwk.x);
  const y = base64UrlToBytes(jwk.y);
  if (x.length !== 32 || y.length !== 32) {
    throw new Error("Invalid P-256 public key coordinate length.");
  }
  const publicKey = new Uint8Array(65);
  publicKey[0] = 0x04;
  publicKey.set(x, 1);
  publicKey.set(y, 33);
  return publicKey;
}

type DpopProofInput = {
  readonly proof: string | null | undefined;
  readonly method: string;
  readonly url: string;
  readonly nowEpochSeconds: number;
  readonly expectedThumbprint?: string;
  readonly expectedAccessToken?: string;
  readonly maxAgeSeconds?: number;
};

type DpopPreparedProof = {
  readonly thumbprint: string;
  readonly jti: string;
  readonly iat: number;
  readonly jwk: DpopPublicJwkType;
  // `ArrayBuffer`-backed views so SubtleCrypto accepts them as BufferSource.
  readonly signature: Uint8Array<ArrayBuffer>;
  readonly signingInput: Uint8Array<ArrayBuffer>;
};

type DpopProofPreparation =
  | { readonly ok: true; readonly proof: DpopPreparedProof }
  | Extract<DpopVerificationResult, { readonly ok: false }>;

function finishDpopProofVerification(
  proof: DpopPreparedProof,
  input: DpopProofInput,
): DpopVerificationResult {
  const maxAgeSeconds = input.maxAgeSeconds ?? DEFAULT_MAX_AGE_SECONDS;
  if (proof.iat > input.nowEpochSeconds + 5 || input.nowEpochSeconds - proof.iat > maxAgeSeconds) {
    return {
      ok: false,
      code: "time_window",
      reason: "DPoP proof is outside the allowed time window.",
    };
  }
  return { ok: true, thumbprint: proof.thumbprint, jti: proof.jti, iat: proof.iat };
}

function prepareDpopProof(input: DpopProofInput): DpopProofPreparation {
  if (!input.proof?.trim()) {
    return { ok: false, code: "missing_proof", reason: "Missing DPoP proof." };
  }

  const parts = input.proof.split(".");
  if (parts.length !== 3 || !parts[0] || !parts[1] || !parts[2]) {
    return { ok: false, code: "malformed_proof", reason: "Invalid DPoP compact JWT." };
  }

  try {
    const header = decodeBase64UrlDpopJwtHeader(parts[0]);
    const payload = decodeBase64UrlDpopJwtPayload(parts[1]);
    if (Option.isNone(header)) {
      return { ok: false, code: "malformed_proof", reason: "Invalid DPoP JWT header." };
    }
    if (Option.isNone(payload)) {
      return { ok: false, code: "malformed_proof", reason: "Invalid DPoP JWT payload." };
    }

    const thumbprint = computeDpopJwkThumbprint(header.value.jwk);
    if (input.expectedThumbprint && thumbprint !== input.expectedThumbprint) {
      return { ok: false, code: "key_mismatch", reason: "DPoP key thumbprint mismatch." };
    }
    if (payload.value.htm.toUpperCase() !== input.method.toUpperCase()) {
      return { ok: false, code: "method_mismatch", reason: "DPoP method mismatch." };
    }
    const normalizedHtu = normalizeDpopHtu(input.url);
    if (normalizedHtu === null || payload.value.htu !== normalizedHtu) {
      return { ok: false, code: "url_mismatch", reason: "DPoP URL mismatch." };
    }
    if (input.expectedAccessToken) {
      const expectedAth = computeDpopAccessTokenHash(input.expectedAccessToken);
      if (payload.value.ath !== expectedAth) {
        return {
          ok: false,
          code: "access_token_hash_mismatch",
          reason: "DPoP access token hash mismatch.",
        };
      }
    }

    return {
      ok: true,
      proof: {
        thumbprint,
        jti: payload.value.jti,
        iat: payload.value.iat,
        jwk: header.value.jwk,
        // Copy so the buffer type stays `Uint8Array<ArrayBuffer>` for SubtleCrypto.
        signature: new Uint8Array(base64UrlToBytes(parts[2])),
        signingInput: new Uint8Array(new TextEncoder().encode(`${parts[0]}.${parts[1]}`)),
      },
    };
  } catch {
    return { ok: false, code: "invalid_proof", reason: "Invalid DPoP proof." };
  }
}

/**
 * Synchronous verification used on Node runtimes. The pure-JS P-256 signature
 * check costs several milliseconds of CPU; runtimes with a native `SubtleCrypto`
 * (Cloudflare Workers, browsers) should use `verifyDpopProofAsync` instead.
 */
export function verifyDpopProof(input: DpopProofInput): DpopVerificationResult {
  const prepared = prepareDpopProof(input);
  if (!prepared.ok) {
    return prepared;
  }
  try {
    const verified = p256.verify(
      prepared.proof.signature,
      sha256(prepared.proof.signingInput),
      publicKeyBytesFromJwk(prepared.proof.jwk),
      {
        prehash: false,
        format: "compact",
      },
    );
    if (!verified) {
      return { ok: false, code: "invalid_signature", reason: "Invalid DPoP signature." };
    }
  } catch {
    return { ok: false, code: "invalid_proof", reason: "Invalid DPoP proof." };
  }
  return finishDpopProofVerification(prepared.proof, input);
}

/**
 * WebCrypto variant of `verifyDpopProof` for CPU-constrained runtimes: the ES256
 * signature check runs in native `subtle.verify` instead of pure-JS arithmetic.
 */
export async function verifyDpopProofAsync(
  input: DpopProofInput,
  subtle: SubtleCrypto,
): Promise<DpopVerificationResult> {
  const prepared = prepareDpopProof(input);
  if (!prepared.ok) {
    return prepared;
  }
  try {
    const key = await subtle.importKey(
      "jwk",
      prepared.proof.jwk,
      { name: "ECDSA", namedCurve: "P-256" },
      false,
      ["verify"],
    );
    const verified = await subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      key,
      prepared.proof.signature,
      prepared.proof.signingInput,
    );
    if (!verified) {
      return { ok: false, code: "invalid_signature", reason: "Invalid DPoP signature." };
    }
  } catch {
    return { ok: false, code: "invalid_proof", reason: "Invalid DPoP proof." };
  }
  return finishDpopProofVerification(prepared.proof, input);
}
