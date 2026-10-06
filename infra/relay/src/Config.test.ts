import { assert, expect, it } from "@effect/vitest";
import * as ConfigProvider from "effect/ConfigProvider";
import * as Effect from "effect/Effect";
import * as Redacted from "effect/Redacted";
import * as Option from "effect/Option";

import * as RelayConfiguration from "./Config.ts";

it.effect("returns null when APNS_ENABLED is false", () =>
  Effect.gen(function* () {
    const creds = yield* RelayConfiguration.resolveApnsCredentials.pipe(
      Effect.provide(
        ConfigProvider.layer(
          ConfigProvider.fromEnv({
            env: {
              APNS_ENABLED: "false",
              APNS_ENVIRONMENT: "production",
              APNS_TEAM_ID: "TEAM123",
              APNS_KEY_ID: "KEY123",
              APNS_BUNDLE_ID: "club.touchtech.s5code",
              APNS_PRIVATE_KEY: "secret-key",
            },
          }),
        ),
      ),
    );
    assert.isNull(creds);
  }),
);

it.effect("returns null when APNS env vars are empty or missing", () =>
  Effect.gen(function* () {
    const creds = yield* RelayConfiguration.resolveApnsCredentials.pipe(
      Effect.provide(
        ConfigProvider.layer(
          ConfigProvider.fromEnv({
            env: {
              APNS_ENVIRONMENT: "",
              APNS_TEAM_ID: "",
              APNS_KEY_ID: "",
              APNS_BUNDLE_ID: "",
              APNS_PRIVATE_KEY: "",
            },
          }),
        ),
      ),
    );
    assert.isNull(creds);
  }),
);

it.effect("returns null when APNS_ENVIRONMENT is invalid", () =>
  Effect.gen(function* () {
    const creds = yield* RelayConfiguration.resolveApnsCredentials.pipe(
      Effect.provide(
        ConfigProvider.layer(
          ConfigProvider.fromEnv({
            env: {
              APNS_ENVIRONMENT: "staging",
              APNS_TEAM_ID: "TEAM123",
              APNS_KEY_ID: "KEY123",
              APNS_BUNDLE_ID: "club.touchtech.s5code",
              APNS_PRIVATE_KEY: "secret-key",
            },
          }),
        ),
      ),
    );
    assert.isNull(creds);
  }),
);

it.effect("resolves valid APNS credentials when all required env vars are present", () =>
  Effect.gen(function* () {
    const creds = yield* RelayConfiguration.resolveApnsCredentials.pipe(
      Effect.provide(
        ConfigProvider.layer(
          ConfigProvider.fromEnv({
            env: {
              APNS_ENVIRONMENT: "production",
              APNS_TEAM_ID: "TEAM123",
              APNS_KEY_ID: "KEY123",
              APNS_BUNDLE_ID: "club.touchtech.s5code",
              APNS_PRIVATE_KEY: "secret-key",
            },
          }),
        ),
      ),
    );
    assert.isNotNull(creds);
    assert.equal(creds?.environment, "production");
    assert.equal(creds?.teamId, "TEAM123");
    assert.equal(creds?.keyId, "KEY123");
    assert.equal(creds?.bundleId, "club.touchtech.s5code");
    assert.equal(Redacted.value(creds!.privateKey), "secret-key");
  }),
);

it.effect.each([
  { name: "missing", env: {}, expected: "off" },
  { name: "empty", env: { RELAY_TUNNEL_CLEANUP_MODE: "" }, expected: "off" },
  { name: "whitespace", env: { RELAY_TUNNEL_CLEANUP_MODE: "  \t" }, expected: "off" },
  { name: "off", env: { RELAY_TUNNEL_CLEANUP_MODE: "off" }, expected: "off" },
  {
    name: "dry-run",
    env: { RELAY_TUNNEL_CLEANUP_MODE: "dry-run" },
    expected: "dry-run",
  },
  { name: "enabled", env: { RELAY_TUNNEL_CLEANUP_MODE: "enabled" }, expected: "enabled" },
] as const)("loads $name cleanup mode as $expected", ({ env, expected }) =>
  Effect.gen(function* () {
    const provider = ConfigProvider.fromEnv({ env });
    expect(yield* RelayConfiguration.managedEndpointCleanupModeConfig.parse(provider)).toBe(
      expected,
    );
  }),
);

it.effect("rejects an invalid cleanup mode", () =>
  Effect.gen(function* () {
    const provider = ConfigProvider.fromEnv({
      env: { RELAY_TUNNEL_CLEANUP_MODE: "delete-everything" },
    });
    const error = yield* Effect.flip(
      RelayConfiguration.managedEndpointCleanupModeConfig.parse(provider),
    );

    expect(error._tag).toBe("ConfigError");
    expect(error.message).toContain('Expected "off" | "dry-run" | "enabled"');
  }),
);

it.effect("reads the legacy cleanup mode independently of the main one", () =>
  Effect.gen(function* () {
    const provider = ConfigProvider.fromEnv({
      env: { RELAY_TUNNEL_CLEANUP_MODE: "enabled", RELAY_LEGACY_TUNNEL_CLEANUP_MODE: "dry-run" },
    });
    expect(yield* RelayConfiguration.managedEndpointCleanupModeConfig.parse(provider)).toBe(
      "enabled",
    );
    expect(yield* RelayConfiguration.legacyManagedEndpointCleanupModeConfig.parse(provider)).toBe(
      "dry-run",
    );
    expect(
      yield* RelayConfiguration.legacyManagedEndpointCleanupModeConfig.parse(
        ConfigProvider.fromEnv({ env: {} }),
      ),
    ).toBe("off");
  }),
);

it.effect.each([
  { name: "missing", env: {}, expected: Option.none() },
  {
    name: "positive",
    env: { RELAY_LEGACY_TUNNEL_GRACE_MINUTES: "10" },
    expected: Option.some(10),
  },
] as const)("loads a $name legacy grace override", ({ env, expected }) =>
  Effect.gen(function* () {
    const minutes = yield* RelayConfiguration.legacyTunnelGraceMinutesConfig.parse(
      ConfigProvider.fromEnv({ env }),
    );
    expect(minutes).toEqual(expected);
  }),
);

it.effect.each(["0", "-10"])("rejects a grace override of %s minutes", (value) =>
  Effect.gen(function* () {
    const error = yield* Effect.flip(
      RelayConfiguration.legacyTunnelGraceMinutesConfig.parse(
        ConfigProvider.fromEnv({ env: { RELAY_LEGACY_TUNNEL_GRACE_MINUTES: value } }),
      ),
    );
    expect(error._tag).toBe("ConfigError");
  }),
);
