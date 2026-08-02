import { CommandId, type OrchestrationSession, type ThreadId } from "@t3tools/contracts";
import * as Clock from "effect/Clock";
import * as Crypto from "effect/Crypto";
import * as DateTime from "effect/DateTime";
import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Schedule from "effect/Schedule";

import { OrchestrationEngineService } from "../../orchestration/Services/OrchestrationEngine.ts";
import { ProjectionSnapshotQuery } from "../../orchestration/Services/ProjectionSnapshotQuery.ts";
import { forkParked } from "../../serverActivation.ts";
import { ProviderSessionDirectory } from "../Services/ProviderSessionDirectory.ts";
import {
  ProviderSessionReaper,
  type ProviderSessionReaperShape,
} from "../Services/ProviderSessionReaper.ts";
import { ProviderService } from "../Services/ProviderService.ts";

const DEFAULT_INACTIVITY_THRESHOLD_MS = 30 * 60 * 1000;
const DEFAULT_SWEEP_INTERVAL_MS = 5 * 60 * 1000;

export interface ProviderSessionReaperLiveOptions {
  readonly inactivityThresholdMs?: number;
  readonly sweepIntervalMs?: number;
}

const makeProviderSessionReaper = (options?: ProviderSessionReaperLiveOptions) =>
  Effect.gen(function* () {
    const providerService = yield* ProviderService;
    const directory = yield* ProviderSessionDirectory;
    const projectionSnapshotQuery = yield* ProjectionSnapshotQuery;
    const orchestrationEngine = yield* OrchestrationEngineService;
    const crypto = yield* Crypto.Crypto;

    const inactivityThresholdMs = Math.max(
      1,
      options?.inactivityThresholdMs ?? DEFAULT_INACTIVITY_THRESHOLD_MS,
    );
    const sweepIntervalMs = Math.max(1, options?.sweepIntervalMs ?? DEFAULT_SWEEP_INTERVAL_MS);

    const serverCommandId = (tag: string) =>
      crypto.randomUUIDv4.pipe(Effect.map((uuid) => CommandId.make(`server:${tag}:${uuid}`)));

    /** Threads whose provider adapter holds a live session in this process. */
    const listLiveThreadIds = () =>
      providerService
        .listSessions()
        .pipe(Effect.map((sessions) => new Set(sessions.map((session) => session.threadId))));

    const getThreadShell = (threadId: ThreadId) =>
      projectionSnapshotQuery.getThreadShellById(threadId).pipe(Effect.map(Option.getOrUndefined));

    const STRANDED_TURN_REASON =
      "Provider session was lost (server restarted or the provider process exited); the running turn was interrupted.";

    /**
     * Emit a `thread.session.set` with a non-running status so the projector
     * settles the stranded turn and clears the thread's active turn. Without
     * this, a thread whose provider died mid-turn shows as "running" forever:
     * the reaper skips sessions with an active turn and no provider event
     * will ever arrive to settle it.
     */
    const settleStrandedTurn = (input: {
      readonly threadId: ThreadId;
      readonly session: OrchestrationSession;
    }) =>
      Effect.gen(function* () {
        const updatedAt = DateTime.formatIso(yield* DateTime.now);
        yield* orchestrationEngine.dispatch({
          type: "thread.session.set",
          commandId: yield* serverCommandId("provider-session-reaper"),
          threadId: input.threadId,
          session: {
            threadId: input.threadId,
            status: "stopped",
            providerName: input.session.providerName,
            providerInstanceId: input.session.providerInstanceId,
            runtimeMode: input.session.runtimeMode,
            activeTurnId: null,
            lastError: STRANDED_TURN_REASON,
            updatedAt,
          },
          createdAt: updatedAt,
        });
      });

    /**
     * Server start is the one moment where every persisted session is
     * guaranteed to be orphaned: this process never spawned them, and their
     * provider subprocesses died with the previous server. Settle any turn
     * still marked active immediately instead of waiting out the
     * inactivity-threshold sweep.
     */
    const reconcileOrphanedSessionsAtStartup = Effect.gen(function* () {
      const bindings = yield* directory.listBindings();
      const liveThreadIds = yield* listLiveThreadIds();
      for (const binding of bindings) {
        if (binding.status === "stopped") {
          continue;
        }
        if (liveThreadIds.has(binding.threadId)) {
          continue;
        }
        const thread = yield* getThreadShell(binding.threadId);
        const session = thread?.session ?? null;
        if (session === null || session.activeTurnId === null) {
          continue;
        }
        yield* settleStrandedTurn({ threadId: binding.threadId, session }).pipe(
          Effect.tap(() =>
            Effect.logInfo("provider.session.reaper.settled-stranded-turn", {
              threadId: binding.threadId,
              turnId: session.activeTurnId,
              phase: "startup",
            }),
          ),
          Effect.catchCause((cause) =>
            Effect.logWarning("provider.session.reaper.settle-failed", {
              threadId: binding.threadId,
              turnId: session.activeTurnId,
              phase: "startup",
              cause,
            }),
          ),
        );
      }
    });

    const sweep = Effect.gen(function* () {
      const bindings = yield* directory.listBindings();
      const liveThreadIds = yield* listLiveThreadIds();
      const now = yield* Clock.currentTimeMillis;
      let reapedCount = 0;

      for (const binding of bindings) {
        if (binding.status === "stopped") {
          continue;
        }

        const lastSeenMs = Date.parse(binding.lastSeenAt);
        if (Number.isNaN(lastSeenMs)) {
          yield* Effect.logWarning("provider.session.reaper.invalid-last-seen", {
            threadId: binding.threadId,
            provider: binding.provider,
            lastSeenAt: binding.lastSeenAt,
          });
          continue;
        }

        const idleDurationMs = now - lastSeenMs;
        if (idleDurationMs < inactivityThresholdMs) {
          continue;
        }

        const thread = yield* getThreadShell(binding.threadId);
        const session = thread?.session ?? null;
        const hasActiveTurn = session?.activeTurnId != null;
        const isLive = liveThreadIds.has(binding.threadId);

        if (isLive && hasActiveTurn) {
          // A live provider session can idle past the threshold mid-turn
          // (long tool calls); leave it alone.
          yield* Effect.logDebug("provider.session.reaper.skipped-active-turn", {
            threadId: binding.threadId,
            activeTurnId: session?.activeTurnId,
            idleDurationMs,
          });
          continue;
        }

        if (!isLive && hasActiveTurn && session !== null) {
          // The provider process is gone but the projection still shows the
          // turn running — settle it before reaping so the thread unsticks.
          yield* settleStrandedTurn({
            threadId: binding.threadId,
            session,
          }).pipe(
            Effect.tap(() =>
              Effect.logInfo("provider.session.reaper.settled-stranded-turn", {
                threadId: binding.threadId,
                turnId: session.activeTurnId,
              }),
            ),
            Effect.catchCause((cause) =>
              Effect.logWarning("provider.session.reaper.settle-failed", {
                threadId: binding.threadId,
                turnId: session.activeTurnId,
                cause,
              }),
            ),
          );
        }

        const reaped = yield* providerService.stopSession({ threadId: binding.threadId }).pipe(
          Effect.tap(() =>
            Effect.logInfo("provider.session.reaped", {
              threadId: binding.threadId,
              provider: binding.provider,
              idleDurationMs,
              reason: "inactivity_threshold",
            }),
          ),
          Effect.as(true),
          Effect.catchCause((cause) =>
            Effect.logWarning("provider.session.reaper.stop-failed", {
              threadId: binding.threadId,
              provider: binding.provider,
              idleDurationMs,
              cause,
            }).pipe(Effect.as(false)),
          ),
        );

        if (reaped) {
          reapedCount += 1;
        }
      }

      if (reapedCount > 0) {
        yield* Effect.logInfo("provider.session.reaper.sweep-complete", {
          reapedCount,
          totalBindings: bindings.length,
        });
      }
    });

    const start: ProviderSessionReaperShape["start"] = () =>
      Effect.gen(function* () {
        yield* reconcileOrphanedSessionsAtStartup.pipe(
          Effect.catchCause((cause) =>
            Effect.logWarning("provider.session.reaper.startup-reconcile-failed", {
              cause,
            }),
          ),
        );

        yield* forkParked(
          sweep.pipe(
            Effect.catch((error: unknown) =>
              Effect.logWarning("provider.session.reaper.sweep-failed", {
                error,
              }),
            ),
            Effect.catchDefect((defect: unknown) =>
              Effect.logWarning("provider.session.reaper.sweep-defect", {
                defect,
              }),
            ),
            Effect.repeat(Schedule.spaced(Duration.millis(sweepIntervalMs))),
          ),
        );

        yield* Effect.logInfo("provider.session.reaper.started", {
          inactivityThresholdMs,
          sweepIntervalMs,
        });
      });

    return {
      start,
    } satisfies ProviderSessionReaperShape;
  });

export const makeProviderSessionReaperLive = (options?: ProviderSessionReaperLiveOptions) =>
  Layer.effect(ProviderSessionReaper, makeProviderSessionReaper(options));

export const ProviderSessionReaperLive = makeProviderSessionReaperLive();
