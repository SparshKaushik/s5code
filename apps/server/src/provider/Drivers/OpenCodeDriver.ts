/**
 * OpenCodeDriver — `ProviderDriver` for OpenCode (v2).
 *
 * Bundles `snapshot`, `adapter`, and `textGeneration` using an OpenCode
 * HTTP service client per provider instance.
 *
 * @module provider/Drivers/OpenCodeDriver
 */
import { OpenCodeSettings, ProviderDriverKind } from "@t3tools/contracts";
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
import { makeOpenCodeAdapter } from "../Layers/OpenCodeAdapter.ts";
import { readOpenCodeGoUsageLimits } from "../Layers/openCodeUsageLimits.ts";
import {
  checkOpenCodeProviderStatus,
  makePendingOpenCodeProvider,
  openCodeSkillsToServerProviderSkills,
  openCodeCommandsToServerProviderSlashCommands,
} from "../Layers/OpenCodeProvider.ts";
import { makeManagedServerProvider } from "../makeManagedServerProvider.ts";
import { makeOpenCodeHost } from "../OpenCodeHost.ts";
import {
  defaultProviderContinuationIdentity,
  type ProviderDriver,
  type ProviderInstance,
} from "../ProviderDriver.ts";
import { withInstanceIdentity } from "./instanceIdentity.ts";
import { mergeProviderInstanceEnvironment } from "../ProviderInstanceEnvironment.ts";
import { makeOpenCodeTextGeneration } from "../../textGeneration/OpenCodeTextGeneration.ts";
import {
  haveProviderSnapshotSettingsChanged,
  makeProviderSnapshotSettingsSource,
  type ProviderSnapshotSettings,
} from "../providerUpdateSettings.ts";

const decodeOpenCodeSettings = Schema.decodeSync(OpenCodeSettings);
const DRIVER_KIND = ProviderDriverKind.make("opencode");

export type OpenCodeDriverEnv =
  | BackgroundPolicy.BackgroundPolicy
  | Crypto.Crypto
  | FileSystem.FileSystem
  | HttpClient.HttpClient
  | Path.Path
  | ServerConfig
  | ServerSettingsService;

export const OpenCodeDriver: ProviderDriver<OpenCodeSettings, OpenCodeDriverEnv> = {
  driverKind: DRIVER_KIND,
  metadata: {
    displayName: "OpenCode",
    supportsMultipleInstances: true,
  },
  configSchema: OpenCodeSettings,
  defaultConfig: (): OpenCodeSettings => decodeOpenCodeSettings({}),
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
        displayName: displayName ?? "OpenCode",
        accentColor,
        continuationGroupKey: continuationIdentity.continuationKey,
      });

      const effectiveConfig = { ...config, enabled } satisfies OpenCodeSettings;

      const hostHandle = yield* makeOpenCodeHost({
        instanceId,
        config: effectiveConfig,
        defaultDirectory: serverConfig.cwd,
        stateDir: serverConfig.stateDir,
        environment: processEnv,
      });

      const adapter = yield* makeOpenCodeAdapter(hostHandle, {
        instanceId,
      });

      const textGeneration = makeOpenCodeTextGeneration(hostHandle, effectiveConfig);

      const fileSystem = yield* FileSystem.FileSystem;
      const pathService = yield* Path.Path;
      const httpClient = yield* HttpClient.HttpClient;

      const checkProvider = Effect.all(
        {
          provider: checkOpenCodeProviderStatus(hostHandle, effectiveConfig, serverConfig.cwd),
          usageLimits: readOpenCodeGoUsageLimits({
            enabled: effectiveConfig.enabled,
            serverUrl: effectiveConfig.serverUrl,
            environment: processEnv,
          }),
        },
        { concurrency: "unbounded" },
      ).pipe(
        Effect.map(({ provider, usageLimits }) => ({ ...provider, usageLimits })),
        Effect.map(stampIdentity),
        Effect.provideService(FileSystem.FileSystem, fileSystem),
        Effect.provideService(Path.Path, pathService),
        Effect.provideService(HttpClient.HttpClient, httpClient),
      );
      // Per-workspace inventory rides the host client's `location` parameter,
      // so one daemon serves any project directory.
      const loadWorkspaceForCwd = (cwd: string) =>
        Effect.tryPromise({
          try: async () => {
            const [commandsRes, skillsRes] = await Promise.all([
              hostHandle.client.command
                .list({ location: { directory: cwd } })
                .catch(() => ({ data: [] })),
              hostHandle.client.skill
                .list({ location: { directory: cwd } })
                .catch(() => ({ data: [] })),
            ]);
            return {
              commands: (commandsRes as { data?: unknown[] }).data ?? [],
              skills: (skillsRes as { data?: unknown[] }).data ?? [],
            };
          },
          catch: (cause) =>
            new ProviderDriverError({
              driver: DRIVER_KIND,
              instanceId,
              detail: `Failed to probe OpenCode commands and skills for '${cwd}'`,
              cause,
            }),
        }).pipe(Effect.timeout("20 seconds"));

      const snapshotSettings = makeProviderSnapshotSettingsSource(effectiveConfig, serverSettings);

      const snapshot = yield* makeManagedServerProvider<ProviderSnapshotSettings<OpenCodeSettings>>(
        {
          resolveMaintenance: () =>
            Effect.succeed({
              provider: DRIVER_KIND,
              packageName: "@opencode-ai/client-v2",
              canUpdate: false,
              status: "current" as const,
              update: null,
            }),
          getSettings: snapshotSettings.getSettings,
          streamSettings: snapshotSettings.streamSettings,
          haveSettingsChanged: haveProviderSnapshotSettingsChanged,
          checkProviderOnSettingsChange: () => false,
          refreshOnInterval: false,
          initialSnapshot: (settings) =>
            makePendingOpenCodeProvider(settings.provider).pipe(Effect.map(stampIdentity)),
          checkProvider,
        },
      ).pipe(
        Effect.mapError(
          (cause) =>
            new ProviderDriverError({
              driver: DRIVER_KIND,
              instanceId,
              detail: `Failed to build OpenCode snapshot: ${cause.message ?? String(cause)}`,
              cause,
            }),
        ),
      );

      return {
        instanceId,
        driverKind: DRIVER_KIND,
        continuationIdentity,
        displayName,
        accentColor,
        enabled,
        snapshot,
        snapshotForCwd: (cwd) =>
          !effectiveConfig.enabled
            ? snapshot.getSnapshot
            : Effect.all([snapshot.getSnapshot, loadWorkspaceForCwd(cwd)]).pipe(
                Effect.map(([machineSnapshot, { skills, commands }]) => ({
                  ...machineSnapshot,
                  skills: openCodeSkillsToServerProviderSkills(skills),
                  slashCommands: openCodeCommandsToServerProviderSlashCommands(commands),
                })),
                Effect.mapError((cause) =>
                  Schema.is(ProviderDriverError)(cause)
                    ? cause
                    : new ProviderDriverError({
                        driver: DRIVER_KIND,
                        instanceId,
                        detail: `Failed to probe OpenCode commands and skills for '${cwd}'`,
                        cause,
                      }),
                ),
              ),
        adapter,
        textGeneration,
      } satisfies ProviderInstance;
    }),
};
