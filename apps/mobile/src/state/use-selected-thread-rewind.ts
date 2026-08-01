import {
  describeRewindStepFailure,
  describeRewindStepResult,
  type RewindDirection,
} from "@t3tools/client-runtime/state/rewind";
import {
  isAtomCommandInterrupted,
  squashAtomCommandFailure,
} from "@t3tools/client-runtime/state/runtime";
import type { RewindStatus } from "@t3tools/contracts";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  buildRewindMenuRows,
  type RewindMenuRow,
} from "../features/threads/threadRewindPresentation";
import { useEnvironmentQuery } from "./query";
import { rewindEnvironment } from "./rewind";
import { useAtomCommand } from "./use-atom-command";
import { showThreadActionResult } from "./use-vcs-action-state";
import { useThreadSelection } from "./use-thread-selection";

export interface SelectedThreadRewindState {
  readonly status: RewindStatus | null;
  /**
   * False when the experiment is off, the server predates the RPC, or the
   * thread has no snapshottable workspace. Surfaces hide the affordance
   * entirely rather than showing rows that can never act.
   */
  readonly available: boolean;
  readonly rows: ReadonlyArray<RewindMenuRow>;
  readonly stepping: boolean;
  readonly step: (direction: RewindDirection) => void;
}

/**
 * Undo/redo for the selected thread, backed by the server's shadow-git
 * snapshots. Steps are serialized per thread by the shared atom command lane,
 * and outcomes report through the same notification channel as git actions so
 * the thread's progress overlay shows them.
 */
export function useSelectedThreadRewind(input: {
  readonly threadBusy: boolean;
}): SelectedThreadRewindState {
  const { selectedThread } = useThreadSelection();
  const environmentId = selectedThread?.environmentId ?? null;
  const threadId = selectedThread?.id ?? null;
  const [stepping, setStepping] = useState(false);
  const { data: status, refresh } = useEnvironmentQuery(
    environmentId !== null && threadId !== null
      ? rewindEnvironment.status({ environmentId, input: { threadId } })
      : null,
  );
  const runUndo = useAtomCommand(rewindEnvironment.undo, { reportFailure: false });
  const runRedo = useAtomCommand(rewindEnvironment.redo, { reportFailure: false });

  // A completed turn is what creates the next undo step, and the server
  // captures it after the turn settles. Refreshing on the busy -> idle edge
  // keeps the rows current without polling.
  const wasBusyRef = useRef(input.threadBusy);
  useEffect(() => {
    if (wasBusyRef.current && !input.threadBusy) {
      refresh();
    }
    wasBusyRef.current = input.threadBusy;
  }, [input.threadBusy, refresh]);

  const step = useCallback(
    (direction: RewindDirection) => {
      if (environmentId === null || threadId === null) {
        return;
      }
      setStepping(true);
      void (async () => {
        const run = direction === "undo" ? runUndo : runRedo;
        const result = await run({ environmentId, input: { threadId } });
        setStepping(false);
        if (result._tag === "Failure") {
          if (!isAtomCommandInterrupted(result)) {
            showThreadActionResult({
              type: "error",
              ...describeRewindStepFailure({
                direction,
                failure: squashAtomCommandFailure(result),
              }),
            });
          }
          return;
        }
        const notification = describeRewindStepResult({ direction, result: result.value });
        if (notification) {
          showThreadActionResult({
            type: notification.type === "success" ? "success" : "error",
            title: notification.title,
            description: notification.description,
          });
        }
        refresh();
      })();
    },
    [environmentId, refresh, runRedo, runUndo, threadId],
  );

  return {
    status,
    available: status?.available ?? false,
    rows: buildRewindMenuRows({ status, threadBusy: input.threadBusy, stepping }),
    stepping,
    step,
  };
}
