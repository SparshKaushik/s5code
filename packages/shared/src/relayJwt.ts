import { decodeJwt, importPKCS8, importSPKI, jwtVerify, SignJWT, type JWTPayload } from "jose";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Predicate from "effect/Predicate";
import * as Schema from "effect/Schema";

export const RELAY_LINK_PROOF_TYP = "t3-env-link+jwt";
export const RELAY_MINT_REQUEST_TYP = "t3-cloud-mint+jwt";
export const RELAY_HEALTH_REQUEST_TYP = "t3-cloud-health+jwt";
export const RELAY_MINT_RESPONSE_TYP = "t3-env-mint+jwt";
export const RELAY_HEALTH_RESPONSE_TYP = "t3-env-health+jwt";
export const RELAY_ACTIVITY_PUBLISH_TYP = "t3-env-activity+jwt";
export const RELAY_MANAGED_TUNNEL_RECOVERY_TYP = "t3-env-managed-tunnel-recovery+jwt";
export const RELAY_HOOK_DELIVERY_TYP = "t3-relay-hook-delivery+jwt";
/** Header carrying the signed proof that a webhook request came from the relay. */
export const RELAY_HOOK_DELIVERY_HEADER = "x-t3-relay-delivery";

export class RelayJwtError extends Schema.TaggedError<RelayJwtError>()("RelayJwtError", {
  operation: Schema.Literals(["sign", "verify"]),
  typ: Schema.String,
  issuer: Schema.optional(Schema.String),
  audience: Schema.optional(Schema.String),
  cause: Schema.Defect(),
}) {
  override get message(): string {
    return `Failed to ${this.operation} relay JWT of type "${this.typ}".`;
  }

  static diagnosticCode(error: RelayJwtError): string {
    if (
      Predicate.isObject(error.cause) &&
      Predicate.hasProperty(error.cause, "code") &&
      Predicate.isString(error.cause.code) &&
      error.cause.code.length > 0
    ) {
      return error.cause.code;
    }

    return error.cause instanceof Error && error.cause.name ? error.cause.name : "unknown";
  }
}

export function normalizeRelayIssuer(value: string): string {
  return value.trim().replace(/\/+$/gu, "");
}

export function decodeRelayJwt(token: string): JWTPayload {
  return decodeJwt(token);
}

function normalizePem(value: string): string {
  return value.replace(/\\n/gu, "\n").trim();
}

// PEM parsing plus `subtle.importKey` cost real CPU per call; keys are stable
// values, so reuse imported keys between JWT operations. Bounded so a caller
// that hands us many distinct keys cannot grow the map without limit.
type ImportedRelayKey = Awaited<ReturnType<typeof importSPKI>>;
const importedKeyCache = new Map<string, Promise<ImportedRelayKey>>();
const IMPORTED_KEY_CACHE_LIMIT = 256;

function importEdDsaKeyCached(kind: "pkcs8" | "spki", pem: string) {
  const cacheKey = `${kind}|${pem}`;
  const cached = importedKeyCache.get(cacheKey);
  if (cached !== undefined) {
    return cached;
  }
  const imported = (
    kind === "pkcs8" ? importPKCS8(pem, "EdDSA") : importSPKI(pem, "EdDSA")
  ) as Promise<ImportedRelayKey>;
  importedKeyCache.set(cacheKey, imported);
  if (importedKeyCache.size > IMPORTED_KEY_CACHE_LIMIT) {
    const oldest = importedKeyCache.keys().next().value;
    if (oldest !== undefined) {
      importedKeyCache.delete(oldest);
    }
  }
  return imported;
}

export function signRelayJwt(input: {
  readonly privateKey: string;
  readonly typ: string;
  readonly payload: JWTPayload;
}): Effect.Effect<string, RelayJwtError> {
  return Effect.tryPromise({
    try: async () => {
      const key = await importEdDsaKeyCached("pkcs8", normalizePem(input.privateKey));
      return new SignJWT(input.payload)
        .setProtectedHeader({ alg: "EdDSA", typ: input.typ })
        .sign(key);
    },
    catch: (cause) => new RelayJwtError({ operation: "sign", typ: input.typ, cause }),
  });
}

export function verifyRelayJwt(input: {
  readonly publicKey: string;
  readonly token: string;
  readonly typ: string;
  readonly issuer: string;
  readonly audience: string;
  readonly nowEpochSeconds: number;
  readonly maxTokenAge?: string | number;
}): Effect.Effect<JWTPayload, RelayJwtError> {
  return Effect.tryPromise({
    try: async () => {
      const key = await importEdDsaKeyCached("spki", normalizePem(input.publicKey));
      const verified = await jwtVerify(input.token, key, {
        algorithms: ["EdDSA"],
        typ: input.typ,
        issuer: input.issuer,
        audience: input.audience,
        maxTokenAge: input.maxTokenAge ?? "5 minutes",
        clockTolerance: 60,
        currentDate: DateTime.toDate(DateTime.makeUnsafe(input.nowEpochSeconds * 1_000)),
      });
      return verified.payload;
    },
    catch: (cause) =>
      new RelayJwtError({
        operation: "verify",
        typ: input.typ,
        issuer: input.issuer,
        audience: input.audience,
        cause,
      }),
  });
}
