import {
  MessageId,
  ThreadId,
  TurnId,
  type RewindEntry,
  type RewindStatus,
} from "@t3tools/contracts";
import { describe, expect, it } from "vite-plus/test";

import { buildRewindMenuRows, resolveRewindMenuDirection } from "./threadRewindPresentation";

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

describe("buildRewindMenuRows", () => {
  it("labels each direction with the turn it would move", () => {
    const rows = buildRewindMenuRows({
      status: status({ undo: entry() }),
      threadBusy: false,
      stepping: false,
    });
    expect(rows.map((row) => row.id)).toEqual(["rewind-undo", "rewind-redo"]);
    expect(rows[0]).toMatchObject({
      direction: "undo",
      label: "Undo",
      detail: '"Rename the helper" · 2 files',
      disabled: false,
      icon: "arrow.uturn.backward",
    });
    expect(rows[1]).toMatchObject({
      direction: "redo",
      detail: "Nothing to redo",
      disabled: true,
      icon: "arrow.uturn.forward",
    });
  });

  it("blocks steps while the thread's turn is running and says why", () => {
    const rows = buildRewindMenuRows({
      status: status({ undo: entry() }),
      threadBusy: true,
      stepping: false,
    });
    expect(rows[0].disabled).toBe(true);
    expect(rows[0].detail).toBe("Wait for the current turn to finish");
    // A direction with nothing to move keeps its own reason.
    expect(rows[1].detail).toBe("Nothing to redo");
  });

  it("blocks both directions while a step is in flight", () => {
    const rows = buildRewindMenuRows({
      status: status({ undo: entry(), redo: entry({ state: "undone" }) }),
      threadBusy: false,
      stepping: true,
    });
    expect(rows.every((row) => row.disabled)).toBe(true);
    expect(rows.every((row) => row.detail === "Rewinding…")).toBe(true);
  });

  it("reports unavailable rows when the server has rewind turned off", () => {
    const rows = buildRewindMenuRows({
      status: status({ available: false }),
      threadBusy: false,
      stepping: false,
    });
    expect(rows.every((row) => row.disabled)).toBe(true);
    expect(rows[0].detail).toBe("Unavailable for this thread");
  });
});

describe("resolveRewindMenuDirection", () => {
  it("maps row ids back to directions and ignores foreign ids", () => {
    expect(resolveRewindMenuDirection("rewind-undo")).toBe("undo");
    expect(resolveRewindMenuDirection("rewind-redo")).toBe("redo");
    expect(resolveRewindMenuDirection("terminal-new")).toBeNull();
  });
});
