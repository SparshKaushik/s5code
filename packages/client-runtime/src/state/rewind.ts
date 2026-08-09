import { WS_METHODS } from "@t3tools/contracts";
import { Atom } from "effect/unstable/reactivity";

import type { EnvironmentRegistry } from "../connection/registry.ts";
import {
  createAtomCommandScheduler,
  createEnvironmentRpcCommand,
  createEnvironmentRpcQueryAtomFamily,
} from "./runtime.ts";

/**
 * Session rewind atoms: per-turn undo/redo backed by server-side shadow-git
 * snapshots.
 *
 * Undo and redo share one serial lane per thread. They mutate the workspace,
 * so overlapping invocations could interleave restores from different turns
 * and leave a mix of both on disk.
 */
export function createRewindEnvironmentAtoms<R, E>(
  runtime: Atom.AtomRuntime<EnvironmentRegistry | R, E>,
) {
  const stepScheduler = createAtomCommandScheduler();
  const stepConcurrency = {
    mode: "serial" as const,
    key: ({ environmentId, input }: { environmentId: string; input: { threadId: string } }) =>
      JSON.stringify([environmentId, input.threadId]),
  };

  return {
    status: createEnvironmentRpcQueryAtomFamily(runtime, {
      label: "environment-data:rewind:status",
      tag: WS_METHODS.rewindGetStatus,
      // Status changes only as a result of a turn completing or an undo/redo,
      // both of which refresh explicitly, so a short stale window is enough.
      staleTimeMs: 2_000,
    }),
    undo: createEnvironmentRpcCommand(runtime, {
      label: "environment-data:rewind:undo",
      tag: WS_METHODS.rewindUndo,
      scheduler: stepScheduler,
      concurrency: stepConcurrency,
    }),
    redo: createEnvironmentRpcCommand(runtime, {
      label: "environment-data:rewind:redo",
      tag: WS_METHODS.rewindRedo,
      scheduler: stepScheduler,
      concurrency: stepConcurrency,
    }),
  };
}

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
        key: ({ environmentId, input }: { environmentId: string; input: { scope: string } }) =>
          JSON.stringify([environmentId, input.scope]),
      },
    }),
  };
}

export * from "./rewindPresentation.ts";
