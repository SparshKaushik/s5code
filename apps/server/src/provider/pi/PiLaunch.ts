/**
 * PiLaunch — argv and environment for a `pi --mode rpc` spawn.
 *
 * One place owns the spawn shape so the provider probe, the adapter, and text
 * generation cannot drift on flags or environment isolation.
 *
 * Instance isolation goes through `PI_CODING_AGENT_DIR`, not `HOME`. pi reads
 * its settings, extensions, packages, and credentials from that directory;
 * overriding `HOME` instead would also relocate the macOS keychain lookup and
 * break stored OAuth credentials — the same trap `ClaudeHome` documents.
 *
 * @module provider/pi/PiLaunch
 */
import type { PiSettings, RuntimeMode } from "@t3tools/contracts";
import { tokenizeCliArgs } from "@t3tools/shared/cliArgs";

import { expandHomePath } from "../../pathExpansion.ts";

/** pi's own env var for its agent directory (see pi `config.ts`). */
export const PI_AGENT_DIR_ENV = "PI_CODING_AGENT_DIR";

/** Runtime mode handed to the bundled `t3-runtime-mode` pi extension. */
export const T3CODE_PI_RUNTIME_MODE_ENV = "T3CODE_PI_RUNTIME_MODE";

/** Escape hatch mirroring `T3CODE_CODEX_LAUNCH_ARGS`. */
export const T3CODE_PI_LAUNCH_ARGS_ENV = "T3CODE_PI_LAUNCH_ARGS";

export interface PiLaunchInput {
  readonly piSettings: Pick<PiSettings, "binaryPath" | "launchArgs" | "agentDirPath">;
  readonly environment?: NodeJS.ProcessEnv | undefined;
  /** Extra flags appended after the user's launch args. */
  readonly extraArgs?: ReadonlyArray<string> | undefined;
  /** Extension files to load in addition to pi's own discovery. */
  readonly extensionPaths?: ReadonlyArray<string> | undefined;
  /**
   * Runtime mode to enforce. Only set for real sessions: it is passed to the
   * bundled runtime-mode extension, which is the only way to gate pi's tools.
   */
  readonly runtimeMode?: RuntimeMode | undefined;
}

export interface PiLaunch {
  readonly command: string;
  readonly args: ReadonlyArray<string>;
  readonly env: NodeJS.ProcessEnv;
}

export function resolvePiLaunchArgs(
  launchArgs: string | undefined,
  environment: NodeJS.ProcessEnv = process.env,
): string {
  return environment[T3CODE_PI_LAUNCH_ARGS_ENV]?.trim() || launchArgs?.trim() || "";
}

export function makePiEnvironment(
  piSettings: Pick<PiSettings, "agentDirPath">,
  baseEnv: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const agentDirPath = piSettings.agentDirPath.trim();
  if (agentDirPath.length === 0) {
    return baseEnv;
  }
  return { ...baseEnv, [PI_AGENT_DIR_ENV]: expandHomePath(agentDirPath) };
}

/** Stable key grouping instances that share pi's on-disk state. */
export function makePiContinuationKey(piSettings: Pick<PiSettings, "agentDirPath">): string {
  const agentDirPath = piSettings.agentDirPath.trim();
  return `pi:agent-dir:${agentDirPath.length > 0 ? expandHomePath(agentDirPath) : "default"}`;
}

export function resolvePiLaunch(input: PiLaunchInput): PiLaunch {
  const environment = input.environment ?? process.env;
  const userArgs = tokenizeCliArgs(resolvePiLaunchArgs(input.piSettings.launchArgs, environment));
  const baseEnv = makePiEnvironment(input.piSettings, environment);
  return {
    command: input.piSettings.binaryPath || "pi",
    args: [
      "--mode",
      "rpc",
      ...(input.extensionPaths ?? []).flatMap((path) => ["--extension", path]),
      ...userArgs,
      ...(input.extraArgs ?? []),
    ],
    env:
      input.runtimeMode === undefined
        ? baseEnv
        : { ...baseEnv, [T3CODE_PI_RUNTIME_MODE_ENV]: input.runtimeMode },
  };
}
