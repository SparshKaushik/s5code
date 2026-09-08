/**
 * OpenCode2Driver — `ProviderDriver` for OpenCode 2.
 *
 * Bundles `snapshot`, `adapter`, and `textGeneration` using an OpenCode
 * HTTP service client per provider instance.
 *
 * @module provider/Drivers/OpenCode2Driver
 */
import { OpenCode2Settings, ProviderDriverKind } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import { HttpClient } from "effect/unstable/http";

import type * as Crypto from "effect/Crypto";
import * as BackgroundPolicy from "../../background/BackgroundPolicy.ts";
import { ServerConfig } from "../../config.ts";
import { ServerSettingsService } from "../../serverSettings.ts";
import { ProviderDriverError } from "../Errors.ts";
import { makeOpenCode2Adapter } from "../Layers/OpenCode2Adapter.ts";
import {
  checkOpenCode2ProviderStatus,
  makePendingOpenCode2Provider,
} from "../Layers/OpenCode2Provider.ts";
import { makeManagedServerProvider } from "../makeManagedServerProvider.ts";
import { makeOpenCode2Host } from "../OpenCode2Host.ts";
import {
  defaultProviderContinuationIdentity,
  type ProviderDriver,
  type ProviderInstance,
} from "../ProviderDriver.ts";
import { withInstanceIdentity } from "./instanceIdentity.ts";
import { mergeProviderInstanceEnvironment } from "../ProviderInstanceEnvironment.ts";
import { makeOpenCode2TextGeneration } from "../../textGeneration/OpenCode2TextGeneration.ts";
import {
  haveProviderSnapshotSettingsChanged,
  makeProviderSnapshotSettingsSource,
  type ProviderSnapshotSettings,
} from "../providerUpdateSettings.ts";

const decodeOpenCode2Settings = Schema.decodeSync(OpenCode2Settings);
const DRIVER_KIND = ProviderDriverKind.make("opencode2");

export type OpenCode2DriverEnv =
  | BackgroundPolicy.BackgroundPolicy
  | Crypto.Crypto
  | FileSystem.FileSystem
  | HttpClient.HttpClient
  | Path.Path
  | ServerConfig
  | ServerSettingsService;

export const OpenCode2Driver: ProviderDriver<OpenCode2Settings, OpenCode2DriverEnv> = {
  driverKind: DRIVER_KIND,
  metadata: {
    displayName: "OpenCode 2",
    supportsMultipleInstances: true,
  },
  configSchema: OpenCode2Settings,
  defaultConfig: (): OpenCode2Settings => decodeOpenCode2Settings({}),
  create: ({ instanceId, displayName, accentColor, environment, enabled, config }) =>
    Effect.gen(function* () {
      const serverConfig = yield* ServerConfig;
      const serverSettings = yield* ServerSettingsService;
      const rawEnv = mergeProviderInstanceEnvironment(environment);
      const processEnv: Record<string, string> = {};
      for (const [key, value] of Object.entries(rawEnv)) {
        if (value !== undefined) {
          processEnv[key] = value;
        }
      }

      const continuationIdentity = defaultProviderContinuationIdentity({
        driverKind: DRIVER_KIND,
        instanceId,
      });

      const stampIdentity = withInstanceIdentity({
        instanceId,
        driverKind: DRIVER_KIND,
        displayName: displayName ?? "OpenCode 2",
        accentColor,
        continuationGroupKey: continuationIdentity.continuationKey,
      });

      const effectiveConfig = { ...config, enabled } satisfies OpenCode2Settings;

      const hostHandle = yield* makeOpenCode2Host({
        instanceId,
        config: effectiveConfig,
        defaultDirectory: serverConfig.cwd,
        stateDir: serverConfig.stateDir,
        environment: processEnv,
      });

      const adapter = yield* makeOpenCode2Adapter(hostHandle, {
        instanceId,
      });

      const textGeneration = makeOpenCode2TextGeneration(hostHandle, effectiveConfig);

      const checkProvider = checkOpenCode2ProviderStatus(
        hostHandle,
        effectiveConfig,
        serverConfig.cwd,
      ).pipe(Effect.map(stampIdentity));

      const snapshotSettings = makeProviderSnapshotSettingsSource(effectiveConfig, serverSettings);

      const snapshot = yield* makeManagedServerProvider<
        ProviderSnapshotSettings<OpenCode2Settings>
      >({
        resolveMaintenance: () =>
          Effect.succeed({
            provider: DRIVER_KIND,
            packageName: "@opencode-ai/client-v2",
            canUpdate: false,
            status: "current" as const,
            update: null,
            latestVersion: null,
            currentVersion: "2.0.0-preview",
            checkedAt: null,
            message: null,
          }),
        getSettings: snapshotSettings.getSettings,
        streamSettings: snapshotSettings.streamSettings,
        haveSettingsChanged: haveProviderSnapshotSettingsChanged,
        checkProviderOnSettingsChange: () => false,
        refreshOnInterval: false,
        initialSnapshot: (settings) =>
          makePendingOpenCode2Provider(settings.provider).pipe(Effect.map(stampIdentity)),
        checkProvider,
      }).pipe(
        Effect.mapError(
          (cause) =>
            new ProviderDriverError({
              driver: DRIVER_KIND,
              instanceId,
              detail: `Failed to build OpenCode 2 snapshot: ${cause.message ?? String(cause)}`,
              cause,
            }),
        ),
      );

      return {
        instanceId,
        driverKind: DRIVER_KIND,
        continuationIdentity,
        displayName: displayName ?? "OpenCode 2",
        accentColor,
        enabled,
        snapshot,
        snapshotForCwd: (cwd) =>
          !effectiveConfig.enabled
            ? snapshot.getSnapshot
            : checkOpenCode2ProviderStatus(hostHandle, effectiveConfig, cwd).pipe(
                Effect.map(stampIdentity),
                Effect.mapError(
                  (cause) =>
                    new ProviderDriverError({
                      driver: DRIVER_KIND,
                      instanceId,
                      detail: `Failed to probe OpenCode 2 inventory for '${cwd}'`,
                      cause,
                    }),
                ),
              ),
        adapter,
        textGeneration,
      } satisfies ProviderInstance;
    }),
};
