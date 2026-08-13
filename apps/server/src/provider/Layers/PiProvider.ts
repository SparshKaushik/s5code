/**
 * PiProvider — availability probe and model catalog for the pi driver.
 *
 * pi has no fixed model list. What a user can run depends entirely on their own
 * pi configuration (`~/.pi/agent/settings.json`, provider extensions, installed
 * packages), so the catalog has to come from the live process via
 * `get_available_models`. That means the probe spawns a short-lived
 * `pi --mode rpc --no-session` and asks. An empty catalog is the authoritative
 * signal that no provider credentials are configured, which is what we report
 * as an auth problem rather than an install problem.
 *
 * The same probe run also collects `get_commands`, so extension commands,
 * prompt templates, and skills show up in the composer's slash menu.
 *
 * @module provider/Layers/PiProvider
 */
import {
  type ModelCapabilities,
  type PiSettings,
  type ServerProvider,
  type ServerProviderModel,
  type ServerProviderSlashCommand,
} from "@t3tools/contracts";
import { causeErrorTag } from "@t3tools/shared/observability";
import { createModelCapabilities } from "@t3tools/shared/model";
import { resolveSpawnCommand } from "@t3tools/shared/shell";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Option from "effect/Option";
import * as Result from "effect/Result";
import { HttpClient } from "effect/unstable/http";
import { ChildProcess, ChildProcessSpawner } from "effect/unstable/process";

import {
  buildServerProvider,
  isCommandMissingCause,
  parseGenericCliVersion,
  providerModelsFromSettings,
  spawnAndCollect,
  type ServerProviderDraft,
} from "../providerSnapshot.ts";
import {
  enrichProviderSnapshotWithVersionAdvisory,
  type ProviderMaintenanceCapabilities,
} from "../providerMaintenance.ts";
import { piServerProviderModel } from "../pi/PiModelSupport.ts";
import { makePiRpcConnection } from "../pi/PiRpcConnection.ts";
import type { PiRpcError } from "../pi/PiRpcErrors.ts";
import { resolvePiLaunch } from "../pi/PiLaunch.ts";
import { PiAvailableModels, PiCommands, type PiCommand } from "../pi/PiRpcSchemas.ts";

const PI_PRESENTATION = {
  displayName: "pi",
  badgeLabel: "Early Access",
  // pi has no server-side plan mode; the interaction-mode toggle would be a
  // control with nothing behind it.
  showInteractionModeToggle: false,
} as const;

const EMPTY_CAPABILITIES: ModelCapabilities = createModelCapabilities({ optionDescriptors: [] });

const VERSION_PROBE_TIMEOUT_MS = 8_000;
const CATALOG_PROBE_TIMEOUT_MS = 45_000;

function piModelsFromSettings(
  customModels: ReadonlyArray<string> | undefined,
  discovered: ReadonlyArray<ServerProviderModel> = [],
): ReadonlyArray<ServerProviderModel> {
  return providerModelsFromSettings(discovered, customModels ?? [], EMPTY_CAPABILITIES);
}

export function buildInitialPiProviderSnapshot(
  piSettings: PiSettings,
): Effect.Effect<ServerProviderDraft> {
  return Effect.gen(function* () {
    const checkedAt = yield* Effect.map(DateTime.now, DateTime.formatIso);
    const models = piModelsFromSettings(piSettings.customModels);

    if (!piSettings.enabled) {
      return buildServerProvider({
        presentation: PI_PRESENTATION,
        enabled: false,
        checkedAt,
        models,
        probe: {
          installed: false,
          version: null,
          status: "warning",
          auth: { status: "unknown" },
          message: "pi is disabled in S5 Code settings.",
        },
      });
    }

    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: true,
      checkedAt,
      models,
      probe: {
        installed: true,
        version: null,
        status: "warning",
        auth: { status: "unknown" },
        message: "Checking pi CLI availability...",
      },
    });
  });
}

const runPiVersionCommand = Effect.fn("runPiVersionCommand")(function* (
  piSettings: PiSettings,
  environment: NodeJS.ProcessEnv,
) {
  const command = piSettings.binaryPath || "pi";
  const spawnCommand = yield* resolveSpawnCommand(command, ["--version"], { env: environment });
  return yield* spawnAndCollect(
    command,
    ChildProcess.make(spawnCommand.command, spawnCommand.args, {
      env: environment,
      shell: spawnCommand.shell,
    }),
  );
});

export interface PiCatalog {
  readonly models: ReadonlyArray<ServerProviderModel>;
  readonly slashCommands: ReadonlyArray<ServerProviderSlashCommand>;
}

/**
 * Built-in pi commands surfaced in the composer that `get_commands` omits.
 *
 * pi's `get_commands` only reports extension commands, prompt templates, and
 * skills — not built-in commands like `/compact`, even though the adapter
 * executes them. Keeping `/compact` here makes the manual-compaction path
 * discoverable in the slash menu.
 */
const PI_BUILTIN_SLASH_COMMANDS: ReadonlyArray<ServerProviderSlashCommand> = [
  {
    name: "compact",
    description: "Summarize the conversation to free up context space",
    input: { hint: "Optional custom instructions for the summary" },
  },
];

function slashCommandsFromPi(
  commands: ReadonlyArray<{ readonly name: string; readonly description?: string | undefined }>,
): ReadonlyArray<ServerProviderSlashCommand> {
  const seen = new Set(PI_BUILTIN_SLASH_COMMANDS.map((command) => command.name));
  const result: Array<ServerProviderSlashCommand> = [...PI_BUILTIN_SLASH_COMMANDS];
  for (const command of commands) {
    const name = command.name.trim();
    if (name.length === 0 || seen.has(name)) {
      continue;
    }
    seen.add(name);
    const description = command.description?.trim();
    result.push({
      name,
      ...(description && description.length > 0 ? { description } : {}),
    });
  }
  return result;
}

/**
 * Spawn a throwaway `pi --mode rpc --no-session` and read its catalog.
 *
 * `--no-session` matters: the probe must not create a session file, or every
 * health refresh would litter the user's pi session history.
 */
export const discoverPiCatalog = (
  piSettings: PiSettings,
  cwd: string,
  environment: NodeJS.ProcessEnv,
): Effect.Effect<PiCatalog, PiRpcError, ChildProcessSpawner.ChildProcessSpawner> =>
  Effect.gen(function* () {
    const launch = resolvePiLaunch({
      piSettings,
      environment,
      extraArgs: ["--no-session"],
    });
    const connection = yield* makePiRpcConnection({
      spawn: {
        command: launch.command,
        args: launch.args,
        cwd,
        env: launch.env,
      },
    });

    const models = yield* connection.requestAs("get_available_models", PiAvailableModels);
    // Command discovery is best-effort: a broken extension must not take the
    // whole provider snapshot down to "error".
    const commands = yield* connection
      .requestAs("get_commands", PiCommands)
      .pipe(Effect.orElseSucceed(() => ({ commands: [] as ReadonlyArray<PiCommand> })));

    return {
      models: models.models.map(piServerProviderModel),
      slashCommands: slashCommandsFromPi(commands.commands),
    } satisfies PiCatalog;
  }).pipe(Effect.scoped);

export const checkPiProviderStatus = Effect.fn("checkPiProviderStatus")(function* (
  piSettings: PiSettings,
  cwd: string,
  environment: NodeJS.ProcessEnv = process.env,
): Effect.fn.Return<ServerProviderDraft, never, ChildProcessSpawner.ChildProcessSpawner> {
  const checkedAt = DateTime.formatIso(yield* DateTime.now);
  const fallbackModels = piModelsFromSettings(piSettings.customModels);

  if (!piSettings.enabled) {
    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: false,
      checkedAt,
      models: fallbackModels,
      probe: {
        installed: false,
        version: null,
        status: "warning",
        auth: { status: "unknown" },
        message: "pi is disabled in S5 Code settings.",
      },
    });
  }

  const versionResult = yield* runPiVersionCommand(piSettings, environment).pipe(
    Effect.timeoutOption(VERSION_PROBE_TIMEOUT_MS),
    Effect.result,
  );

  if (Result.isFailure(versionResult)) {
    const error = versionResult.failure;
    const missing = isCommandMissingCause(error);
    yield* Effect.logWarning("pi CLI health check failed.", { errorTag: error._tag });
    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: true,
      checkedAt,
      models: fallbackModels,
      probe: {
        installed: !missing,
        version: null,
        status: "error",
        auth: { status: "unknown" },
        message: missing
          ? "pi CLI (`pi`) is not installed or not on PATH."
          : "Failed to execute pi CLI health check.",
      },
    });
  }

  if (Option.isNone(versionResult.success)) {
    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: true,
      checkedAt,
      models: fallbackModels,
      probe: {
        installed: true,
        version: null,
        status: "error",
        auth: { status: "unknown" },
        message: "pi CLI is installed but timed out while running `pi --version`.",
      },
    });
  }

  const versionOutput = versionResult.success.value;
  const version = parseGenericCliVersion(`${versionOutput.stdout}\n${versionOutput.stderr}`);
  if (versionOutput.code !== 0) {
    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: true,
      checkedAt,
      models: fallbackModels,
      probe: {
        installed: true,
        version,
        status: "error",
        auth: { status: "unknown" },
        message: "pi CLI is installed but failed to run.",
      },
    });
  }

  const catalogExit = yield* discoverPiCatalog(piSettings, cwd, environment).pipe(
    Effect.timeoutOption(CATALOG_PROBE_TIMEOUT_MS),
    Effect.exit,
  );

  if (Exit.isFailure(catalogExit)) {
    yield* Effect.logWarning("pi model discovery failed.", {
      errorTag: causeErrorTag(catalogExit.cause),
    });
    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: true,
      checkedAt,
      models: fallbackModels,
      probe: {
        installed: true,
        version,
        status: "error",
        auth: { status: "unknown" },
        message: "pi CLI is installed but its RPC startup failed. Check server logs for details.",
      },
    });
  }

  if (Option.isNone(catalogExit.value)) {
    return buildServerProvider({
      presentation: PI_PRESENTATION,
      enabled: true,
      checkedAt,
      models: fallbackModels,
      probe: {
        installed: true,
        version,
        status: "error",
        auth: { status: "unknown" },
        message: `pi CLI is installed but model discovery timed out after ${CATALOG_PROBE_TIMEOUT_MS}ms.`,
      },
    });
  }

  const catalog = catalogExit.value.value;
  const models = piModelsFromSettings(piSettings.customModels, catalog.models);
  // pi resolves credentials per upstream provider, so "no models" is the only
  // signal it gives us that nothing is authenticated.
  const hasModels = catalog.models.length > 0;

  return buildServerProvider({
    presentation: PI_PRESENTATION,
    enabled: true,
    checkedAt,
    models,
    slashCommands: catalog.slashCommands,
    probe: {
      installed: true,
      version,
      status: hasModels ? "ready" : "warning",
      auth: hasModels ? { status: "authenticated", type: "pi" } : { status: "unknown" },
      ...(hasModels
        ? {}
        : {
            message:
              "pi is installed but reported no available models. Configure a provider in pi settings (`pi` then `/settings`).",
          }),
    },
  });
});

export const enrichPiSnapshot = (input: {
  readonly snapshot: ServerProvider;
  readonly maintenanceCapabilities: ProviderMaintenanceCapabilities;
  readonly enableProviderUpdateChecks?: boolean;
  readonly publishSnapshot: (snapshot: ServerProvider) => Effect.Effect<void>;
  readonly httpClient: HttpClient.HttpClient;
}): Effect.Effect<void> =>
  enrichProviderSnapshotWithVersionAdvisory(input.snapshot, input.maintenanceCapabilities, {
    enableProviderUpdateChecks: input.enableProviderUpdateChecks,
  }).pipe(
    Effect.provideService(HttpClient.HttpClient, input.httpClient),
    Effect.flatMap((enrichedSnapshot) => input.publishSnapshot(enrichedSnapshot)),
    Effect.catchCause((cause) =>
      Effect.logWarning("pi version advisory enrichment failed", {
        errorTag: causeErrorTag(cause),
      }),
    ),
    Effect.asVoid,
  );
