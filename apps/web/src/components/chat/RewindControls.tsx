import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Redo2Icon, Undo2Icon } from "lucide-react";
import {
  describeRewindAction,
  describeRewindStepFailure,
  describeRewindStepResult,
} from "@t3tools/client-runtime/state/rewind";
import {
  isAtomCommandInterrupted,
  squashAtomCommandFailure,
} from "@t3tools/client-runtime/state/runtime";
import type { EnvironmentId, ThreadId } from "@t3tools/contracts";

import { useEnvironmentQuery } from "../../state/query";
import { rewindEnvironment } from "../../state/rewind";
import { useAtomCommand } from "../../state/use-atom-command";
import { Button } from "../ui/button";
import { toastManager } from "../ui/toast";
import { Tooltip, TooltipPopup, TooltipTrigger } from "../ui/tooltip";

/**
 * Undo/redo for the session rewind experiment.
 *
 * Renders nothing when the server reports rewind unavailable (experiment off,
 * or an older server that does not implement the RPC), so no dead controls
 * appear. Steps are serialized per thread by the atom command lane.
 */
export const RewindControls = memo(function RewindControls({
  environmentId,
  threadId,
  disabled,
}: {
  environmentId: EnvironmentId;
  threadId: ThreadId;
  disabled: boolean;
}) {
  const [runningDirection, setRunningDirection] = useState<"undo" | "redo" | null>(null);
  const { data: status, refresh } = useEnvironmentQuery(
    rewindEnvironment.status({ environmentId, input: { threadId } }),
  );
  const runUndo = useAtomCommand(rewindEnvironment.undo, { reportFailure: false });
  const runRedo = useAtomCommand(rewindEnvironment.redo, { reportFailure: false });

  // A turn finishing is what creates a new undo step, and the server captures
  // it after the turn settles. Refresh on the working -> idle edge so the
  // control reflects the turn that just ran without polling.
  const wasWorkingRef = useRef(disabled);
  useEffect(() => {
    if (wasWorkingRef.current && !disabled) {
      refresh();
    }
    wasWorkingRef.current = disabled;
  }, [disabled, refresh]);

  const step = useCallback(
    (direction: "undo" | "redo") => {
      setRunningDirection(direction);
      void (async () => {
        const run = direction === "undo" ? runUndo : runRedo;
        const result = await run({ environmentId, input: { threadId } });
        setRunningDirection(null);
        if (result._tag === "Failure") {
          if (!isAtomCommandInterrupted(result)) {
            const failure = describeRewindStepFailure({
              direction,
              failure: squashAtomCommandFailure(result),
            });
            toastManager.add({ type: "error", ...failure });
          }
          return;
        }
        const toast = describeRewindStepResult({ direction, result: result.value });
        if (toast) {
          toastManager.add(toast);
        }
        refresh();
      })();
    },
    [environmentId, refresh, runRedo, runUndo, threadId],
  );

  // An unavailable status is the "experiment off" signal; hide entirely.
  if (status === null || !status.available) {
    return null;
  }

  const undoAction = describeRewindAction({ direction: "undo", status });
  const redoAction = describeRewindAction({ direction: "redo", status });

  return (
    <div className="flex items-center gap-0.5">
      {(
        [
          { direction: "undo" as const, action: undoAction, Icon: Undo2Icon },
          { direction: "redo" as const, action: redoAction, Icon: Redo2Icon },
        ] satisfies ReadonlyArray<{
          direction: "undo" | "redo";
          action: ReturnType<typeof describeRewindAction>;
          Icon: typeof Undo2Icon;
        }>
      ).map(({ direction, action, Icon }) => (
        <Tooltip key={direction}>
          <TooltipTrigger
            render={
              <Button
                type="button"
                size="icon-xs"
                variant="ghost"
                aria-label={action.ariaLabel}
                disabled={disabled || !action.enabled || runningDirection !== null}
                onClick={() => step(direction)}
              />
            }
          >
            <Icon className="size-3.5" />
          </TooltipTrigger>
          <TooltipPopup side="bottom">{action.tooltip}</TooltipPopup>
        </Tooltip>
      ))}
    </div>
  );
});
