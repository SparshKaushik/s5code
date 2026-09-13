import { assert, it } from "@effect/vitest";
import * as ConfigProvider from "effect/ConfigProvider";
import * as Effect from "effect/Effect";
import * as Redacted from "effect/Redacted";

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
