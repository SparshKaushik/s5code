import { WS_METHODS } from "@t3tools/contracts";
import { Atom } from "effect/unstable/reactivity";

import type { EnvironmentRegistry } from "../connection/registry.ts";
import {
  createAtomCommandScheduler,
  createEnvironmentRpcCommand,
  createEnvironmentRpcQueryAtomFamily,
} from "./runtime.ts";

/**
 * Checkpoint storage reporting and cleanup.
 *
 * Cleanup is single-flight per environment: two concurrent sweeps would
 * measure the same refs and double-count what they reclaimed.
 */
export function createCheckpointMaintenanceEnvironmentAtoms<R, E>(
  runtime: Atom.AtomRuntime<EnvironmentRegistry | R, E>,
) {
  return {
    usage: createEnvironmentRpcQueryAtomFamily(runtime, {
      label: "environment-data:checkpoint-maintenance:usage",
      tag: WS_METHODS.checkpointMaintenanceGetUsage,
      // Scanning repositories is not free; the settings panel refreshes
      // explicitly after a cleanup instead of polling.
      staleTimeMs: 30_000,
    }),
    cleanup: createEnvironmentRpcCommand(runtime, {
      label: "environment-data:checkpoint-maintenance:cleanup",
      tag: WS_METHODS.checkpointMaintenanceCleanup,
      scheduler: createAtomCommandScheduler(),
      concurrency: {
        mode: "singleFlight" as const,
        key: ({
          environmentId,
          input,
        }: {
          environmentId: string;
          input: { scope: string; dryRun?: boolean };
        }) => JSON.stringify([environmentId, input.scope, input.dryRun === true]),
      },
    }),
  };
}
