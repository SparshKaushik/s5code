import { describeRewindAction, type RewindDirection } from "@t3tools/client-runtime/state/rewind";
import type { RewindStatus } from "@t3tools/contracts";

export const REWIND_MENU_TITLE = "Session rewind";
export const REWIND_MENU_ACCESSIBILITY_LABEL = "Session rewind";

/** SF Symbol names; both are mapped for Android in `AppSymbol`. */
export type RewindMenuIcon = "arrow.uturn.backward" | "arrow.uturn.forward";

export interface RewindMenuRow {
  readonly id: string;
  readonly direction: RewindDirection;
  /** Control label; the affected turn is described by `detail`. */
  readonly label: string;
  readonly detail: string;
  readonly disabled: boolean;
  readonly icon: RewindMenuIcon;
}

/**
 * The two rewind rows for a thread menu.
 *
 * Undo and redo restore files in the workspace, so they stay disabled while
 * the thread's provider turn is running (the turn would fight the restore) and
 * while another step is still in flight.
 */
export function buildRewindMenuRows(input: {
  readonly status: RewindStatus | null;
  readonly threadBusy: boolean;
  readonly stepping: boolean;
}): ReadonlyArray<RewindMenuRow> {
  const blocked = input.threadBusy || input.stepping;
  return (["undo", "redo"] satisfies ReadonlyArray<RewindDirection>).map((direction) => {
    const action = describeRewindAction({ direction, status: input.status });
    return {
      id: `rewind-${direction}`,
      direction,
      label: direction === "undo" ? "Undo" : "Redo",
      detail: input.stepping
        ? "Rewinding…"
        : input.threadBusy && action.enabled
          ? "Wait for the current turn to finish"
          : action.detail,
      disabled: blocked || !action.enabled,
      icon: direction === "undo" ? "arrow.uturn.backward" : "arrow.uturn.forward",
    };
  });
}

/** Row id back to the direction it steps, for id-keyed Android menus. */
export function resolveRewindMenuDirection(id: string): RewindDirection | null {
  if (id === "rewind-undo") return "undo";
  if (id === "rewind-redo") return "redo";
  return null;
}

/**
 * What a header surface needs to render the rewind affordance, without
 * depending on how the rows were produced.
 */
export interface ThreadRewindMenuModel {
  readonly available: boolean;
  readonly rows: ReadonlyArray<RewindMenuRow>;
  readonly onStep: (direction: RewindDirection) => void;
}
