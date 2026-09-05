import { useAtomValue } from "@effect/atom-react";
import { type VcsActionState, type VcsActionTarget } from "@t3tools/client-runtime/state/vcs";
import { Atom } from "effect/unstable/reactivity";
import { useCallback, useEffect, useRef, useState } from "react";

import { appAtomRegistry } from "./atom-registry";
import { vcsActionManager } from "./vcs";

export function useVcsActionState(target: VcsActionTarget): VcsActionState {
  return useAtomValue(vcsActionManager.stateAtom(target));
}

/**
 * Transient banner state for a completed thread-level action (git action).
 * One channel, so two actions can never stack banners.
 */
export interface ThreadActionResultNotification {
  readonly type: "success" | "error";
  readonly title: string;
  readonly description?: string;
  readonly prUrl?: string;
}

const RESULT_DISMISS_MS = 5_000;

const threadActionResultAtom = Atom.make<ThreadActionResultNotification | null>(null).pipe(
  Atom.keepAlive,
  Atom.withLabel("mobile:thread-action-result"),
);
let dismissTimer: ReturnType<typeof setTimeout> | null = null;

function broadcast(result: ThreadActionResultNotification | null): void {
  appAtomRegistry.set(threadActionResultAtom, result);
}

export function showThreadActionResult(result: ThreadActionResultNotification): void {
  if (dismissTimer) clearTimeout(dismissTimer);
  broadcast(result);
  dismissTimer = setTimeout(() => {
    dismissTimer = null;
    broadcast(null);
  }, RESULT_DISMISS_MS);
}

export function dismissThreadActionResult(): void {
  if (dismissTimer) clearTimeout(dismissTimer);
  dismissTimer = null;
  broadcast(null);
}

export function useThreadActionResultNotification(): {
  readonly result: ThreadActionResultNotification | null;
  readonly dismiss: () => void;
} {
  const result = useAtomValue(threadActionResultAtom);
  return { result, dismiss: dismissThreadActionResult };
}

export type ThreadActionProgressPhase = "idle" | "running" | "success" | "error";

export interface ThreadActionProgress {
  readonly phase: ThreadActionProgressPhase;
  readonly label: string | null;
  readonly description: string | null;
  readonly prUrl?: string;
}

const EMPTY_PROGRESS: ThreadActionProgress = {
  phase: "idle",
  label: null,
  description: null,
};

function formatElapsedSeconds(ms: number | null): string | null {
  if (ms === null) return null;
  const elapsed = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (elapsed < 2) return null;
  return `Running for ${elapsed}s`;
}

/**
 * Live progress for the selected thread's actions: the git action lane's
 * running phase, then whatever terminal result was last reported.
 */
export function useThreadActionProgress(target: VcsActionTarget): ThreadActionProgress {
  const actionState = useVcsActionState(target);
  const { result } = useThreadActionResultNotification();

  const [, forceUpdate] = useState(0);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const startElapsedTimer = useCallback(() => {
    if (intervalRef.current) return;
    intervalRef.current = setInterval(() => forceUpdate((n) => n + 1), 1000);
  }, []);

  const stopElapsedTimer = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  useEffect(() => {
    if (actionState.isRunning) {
      startElapsedTimer();
    } else {
      stopElapsedTimer();
    }
    return stopElapsedTimer;
  }, [actionState.isRunning, startElapsedTimer, stopElapsedTimer]);

  if (actionState.isRunning) {
    const description =
      actionState.lastOutputLine ??
      formatElapsedSeconds(actionState.hookStartedAtMs ?? actionState.phaseStartedAtMs);
    return {
      phase: "running",
      label: actionState.currentLabel,
      description,
    };
  }

  if (result) {
    return {
      phase: result.type,
      label: result.title,
      description: result.description ?? null,
      prUrl: result.prUrl,
    };
  }

  return EMPTY_PROGRESS;
}
