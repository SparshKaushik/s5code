import * as Config from "effect/Config";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Redacted from "effect/Redacted";
import * as Schema from "effect/Schema";

export const ApnsEnvironment = Schema.Literals(["sandbox", "production"]);
export type ApnsEnvironment = typeof ApnsEnvironment.Type;

export const ManagedEndpointCleanupMode = Schema.Literals(["off", "dry-run", "enabled"]);
export type ManagedEndpointCleanupMode = typeof ManagedEndpointCleanupMode.Type;
const decodeManagedEndpointCleanupMode = Schema.decodeUnknownEffect(ManagedEndpointCleanupMode);

export const managedEndpointCleanupModeConfig = Config.String("RELAY_TUNNEL_CLEANUP_MODE").pipe(
  Config.withDefault("off"),
  Config.map((value) => value.trim() || "off"),
  Config.mapEffect((value) =>
    decodeManagedEndpointCleanupMode(value).pipe(
      Effect.mapError((error) => new Config.ConfigError(error)),
    ),
  ),
);

export interface ApnsCredentials {
  readonly teamId: string;
  readonly keyId: string;
  readonly privateKey: Redacted.Redacted<string>;
  readonly bundleId: string;
  readonly environment: ApnsEnvironment;
}

export const resolveApnsCredentials = Effect.gen(function* () {
  const apnsEnabled = yield* Config.Boolean("APNS_ENABLED").pipe(Config.withDefault(true));
  if (!apnsEnabled) return null;

  const apnsEnvironmentRaw = Option.getOrUndefined(
    Option.filter(
      yield* Config.String("APNS_ENVIRONMENT").pipe(Config.option),
      (value) => value.trim().length > 0,
    ),
  );
  const apnsEnvironment =
    apnsEnvironmentRaw === "sandbox" || apnsEnvironmentRaw === "production"
      ? apnsEnvironmentRaw
      : undefined;
  const apnsTeamId = Option.getOrUndefined(
    Option.filter(
      yield* Config.String("APNS_TEAM_ID").pipe(Config.option),
      (value) => value.trim().length > 0,
    ),
  );
  const apnsKeyId = Option.getOrUndefined(
    Option.filter(
      yield* Config.String("APNS_KEY_ID").pipe(Config.option),
      (value) => value.trim().length > 0,
    ),
  );
  const apnsBundleId = Option.getOrUndefined(
    Option.filter(
      yield* Config.String("APNS_BUNDLE_ID").pipe(Config.option),
      (value) => value.trim().length > 0,
    ),
  );
  const apnsPrivateKey = Option.getOrUndefined(
    Option.filter(
      yield* Config.Redacted("APNS_PRIVATE_KEY").pipe(Config.option),
      (value) => Redacted.value(value).trim().length > 0,
    ),
  );

  if (
    apnsEnvironment !== undefined &&
    apnsTeamId !== undefined &&
    apnsKeyId !== undefined &&
    apnsBundleId !== undefined &&
    apnsPrivateKey !== undefined
  ) {
    return {
      environment: apnsEnvironment,
      teamId: apnsTeamId,
      keyId: apnsKeyId,
      bundleId: apnsBundleId,
      privateKey: apnsPrivateKey,
    } satisfies ApnsCredentials;
  }
  return null;
});

export class RelayConfiguration extends Context.Service<
  RelayConfiguration,
  {
    readonly relayIssuer: string;
    readonly apns: ApnsCredentials | null;
    readonly fcmServiceAccount?: Redacted.Redacted<string>;
    readonly clerkSecretKey: Redacted.Redacted<string>;
    readonly clerkPublishableKey: string;
    readonly clerkJwtAudience: string;
    readonly apnsDeliveryJobSigningSecret: Redacted.Redacted<string>;
    readonly cloudMintPrivateKey: Redacted.Redacted<string>;
    readonly cloudMintPublicKey: string;
    readonly managedEndpointBaseDomain: string | undefined;
    readonly managedEndpointNamespace: string | undefined;
    readonly managedEndpointCleanupMode?: ManagedEndpointCleanupMode;
  }
>()("t3code-relay/Config/RelayConfiguration") {}

export const make = (configuration: RelayConfiguration["Service"]) =>
  RelayConfiguration.of(configuration);

export const layer = (configuration: RelayConfiguration["Service"]) =>
  Layer.succeed(RelayConfiguration, make(configuration));
