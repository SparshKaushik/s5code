import {
  MessageId,
  ThreadId,
  TurnId,
  type RewindEntry,
  type RewindStatus,
  type RewindStepResult,
} from "@t3tools/contracts";
import { describe, expect, it } from "vite-plus/test";

import {
  describeRewindAction,
  describeRewindStepFailure,
  describeRewindStepResult,
  formatRewindPromptLabel,
  undonePromptForComposer,
} from "./rewindPresentation.ts";

const threadId = ThreadId.make("thread-1");

function entry(overrides: Partial<RewindEntry> = {}): RewindEntry {
  return {
    threadId,
    turnId: TurnId.make("turn-1"),
    sequence: 0,
    userMessageId: MessageId.make("message-1"),
    assistantMessageId: null,
    prompt: "Rename the helper",
    files: ["src/a.ts", "src/b.ts"],
    state: "applied",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    ...overrides,
  };
}

function status(overrides: Partial<RewindStatus> = {}): RewindStatus {
  return {
    threadId,
    available: true,
    undo: null,
    redo: null,
    appliedCount: 0,
    undoneCount: 0,
    ...overrides,
  };
}

describe("formatRewindPromptLabel", () => {
  it("collapses whitespace and truncates long prompts", () => {
    expect(formatRewindPromptLabel("  fix\n\n  the   bug ")).toBe("fix the bug");
    expect(formatRewindPromptLabel("")).toBe("");
    expect(formatRewindPromptLabel("x".repeat(200))).toHaveLength(60);
    expect(formatRewindPromptLabel("x".repeat(200)).endsWith("…")).toBe(true);
  });
});

describe("describeRewindAction", () => {
  it("disables both directions when rewind is unavailable", () => {
    for (const direction of ["undo", "redo"] as const) {
      const action = describeRewindAction({ direction, status: status({ available: false }) });
      expect(action.enabled).toBe(false);
      expect(action.tooltip).toContain("unavailable");
    }
  });

  it("disables a direction with no target entry", () => {
    expect(describeRewindAction({ direction: "undo", status: status() }).tooltip).toBe(
      "Nothing to undo",
    );
    expect(describeRewindAction({ direction: "redo", status: status() }).tooltip).toBe(
      "Nothing to redo",
    );
  });

  it("labels the target turn by prompt and file count", () => {
    const undo = describeRewindAction({
      direction: "undo",
      status: status({ undo: entry() }),
    });
    expect(undo.enabled).toBe(true);
    expect(undo.tooltip).toBe('Undo "Rename the helper" (2 files)');
    expect(undo.ariaLabel).toBe("Undo Rename the helper");
  });

  it("falls back to a generic label when the prompt is empty", () => {
    const redo = describeRewindAction({
      direction: "redo",
      status: status({ redo: entry({ prompt: "   ", files: ["only.ts"] }) }),
    });
    expect(redo.tooltip).toBe("Redo last turn (1 file)");
  });

  it("treats a missing status as unavailable", () => {
    expect(describeRewindAction({ direction: "undo", status: null }).enabled).toBe(false);
  });
});

describe("describeRewindStepResult", () => {
  it("stays silent on an applied step with no work reported", () => {
    const toast = describeRewindStepResult({
      direction: "undo",
      result: {
        outcome: "applied",
        restoredFiles: ["src/a.ts"],
        prompt: "Rename the helper",
        status: status(),
      },
    });
    expect(toast).toEqual({
      type: "success",
      title: "Turn undone",
      description: 'Restored 1 file from "Rename the helper".',
    });
  });

  it("explains non-applied outcomes", () => {
    const base = { restoredFiles: [] as ReadonlyArray<string>, prompt: null, status: status() };
    expect(
      describeRewindStepResult({
        direction: "redo",
        result: { ...base, outcome: "nothing-to-do" },
      })?.description,
    ).toBe("Nothing to redo.");
    expect(
      describeRewindStepResult({
        direction: "undo",
        result: { ...base, outcome: "unavailable" },
      })?.description,
    ).toContain("turned off");
    expect(
      describeRewindStepResult({ direction: "undo", result: { ...base, outcome: "busy" } })
        ?.description,
    ).toContain("still running");
  });
});

describe("describeRewindStepFailure", () => {
  it("uses the failure message when one is present", () => {
    expect(
      describeRewindStepFailure({ direction: "undo", failure: new Error("shadow store missing") }),
    ).toEqual({ title: "Undo failed", description: "shadow store missing" });
  });

  it("falls back to a reassuring message for opaque failures", () => {
    expect(describeRewindStepFailure({ direction: "redo", failure: "boom" })).toEqual({
      title: "Redo failed",
      description: "The workspace was not changed.",
    });
  });
});

describe("undonePromptForComposer", () => {
  function appliedResult(overrides: Partial<RewindStepResult> = {}): RewindStepResult {
    return {
      outcome: "applied",
      restoredFiles: ["src/a.ts"],
      prompt: "Rename the helper",
      status: status(),
      ...overrides,
    };
  }

  it("returns the prompt for an applied undo", () => {
    expect(undonePromptForComposer({ direction: "undo", result: appliedResult() })).toBe(
      "Rename the helper",
    );
  });

  it("returns null for redo", () => {
    expect(undonePromptForComposer({ direction: "redo", result: appliedResult() })).toBeNull();
  });

  it("returns null for non-applied outcomes", () => {
    for (const outcome of ["nothing-to-do", "unavailable", "busy"] as const) {
      expect(
        undonePromptForComposer({ direction: "undo", result: appliedResult({ outcome }) }),
      ).toBeNull();
    }
  });

  it("returns null when the prompt is missing or whitespace-only", () => {
    expect(
      undonePromptForComposer({ direction: "undo", result: appliedResult({ prompt: null }) }),
    ).toBeNull();
    expect(
      undonePromptForComposer({ direction: "undo", result: appliedResult({ prompt: "   " }) }),
    ).toBeNull();
  });
});
