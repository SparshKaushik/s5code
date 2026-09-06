import { describe, expect, it } from "@effect/vitest";

import {
  piAppendAssistantDelta,
  piAssistantSegmentItemId,
  piAssistantText,
  piCloseAssistantSegment,
  piContentBlocks,
  piContentStreamKind,
  piOpenAssistantSegment,
  piToolItemDetail,
  piToolItemType,
  piToolOutputDelta,
  piToolOutputText,
  piTurnHasAssistantText,
  piTurnStateFromStopReason,
  piUsageSnapshot,
  type PiTurnAssistantSegments,
} from "./PiTurnState.ts";

describe("piToolItemDetail", () => {
  it("recovers the bash command from the args remembered at start", () => {
    expect(piToolItemDetail("bash", { command: "ls -la", timeout: 30 })).toBe("ls -la");
  });

  it("handles string-encoded args", () => {
    expect(piToolItemDetail("bash", JSON.stringify({ command: "git status" }))).toBe("git status");
  });

  it("shows the edited path for file mutations", () => {
    expect(piToolItemDetail("edit", { path: "src/main.ts", oldText: "a", newText: "b" })).toBe(
      "src/main.ts",
    );
    expect(piToolItemDetail("write", { path: "README.md" })).toBe("README.md");
  });

  it("renders nothing for tools with no useful input or absent args", () => {
    expect(piToolItemDetail("bash", undefined)).toBeUndefined();
    expect(piToolItemDetail("bash", {})).toBeUndefined();
    expect(piToolItemDetail("bash", { command: "  " })).toBeUndefined();
    expect(piToolItemDetail("read", {})).toBeUndefined();
    expect(piToolItemDetail(undefined, { command: "ls" })).toBeUndefined();
  });

  it("shows the path for read so the work log names the file", () => {
    expect(piToolItemDetail("read", { path: "src/main.ts", offset: 0, limit: 2000 })).toBe(
      "src/main.ts",
    );
    expect(piToolItemDetail("read", JSON.stringify({ path: "README.md" }))).toBe("README.md");
  });

  it("shows the pattern (and search dir) for grep, and the dir for find/ls", () => {
    expect(piToolItemDetail("grep", { pattern: "TODO", path: "src" })).toBe("TODO in src");
    expect(piToolItemDetail("grep", { pattern: "TODO" })).toBe("TODO");
    expect(piToolItemDetail("find", { pattern: "*.ts", path: "src" })).toBe("src");
    expect(piToolItemDetail("ls", { path: "." })).toBe(".");
    expect(piToolItemDetail("find", { pattern: "*.ts" })).toBeUndefined();
  });

  it("summarizes todo tools so the rows are not bare", () => {
    expect(
      piToolItemDetail("todowrite", {
        todos: [{ content: "a" }, { content: "b" }],
      }),
    ).toBe("2 todos");
    expect(piToolItemDetail("patchtodo", { id: 1, status: "completed" })).toBe("#1 completed");
    expect(piToolItemDetail("patchtodo", { id: 2, priority: "high" })).toBe("#2 high");
    expect(piToolItemDetail("read_todo", {})).toBeUndefined();
    expect(piToolItemDetail("todowrite", { todos: [] })).toBeUndefined();
  });
});

describe("piToolOutputText", () => {
  it("concatenates the text blocks of a tool result's content", () => {
    expect(
      piToolOutputText({
        content: [
          { type: "text", text: "total 48\n" },
          { type: "text", text: "src/\n" },
        ],
        details: {},
      }),
    ).toBe("total 48\nsrc/\n");
  });

  it("returns nothing for a result without content or a non-object result", () => {
    expect(piToolOutputText(undefined)).toBe("");
    expect(piToolOutputText({ details: {} })).toBe("");
    expect(piToolOutputText("loose string")).toBe("");
  });
});

describe("piToolOutputDelta", () => {
  it("emits only the suffix past the previously-seen accumulated text", () => {
    expect(piToolOutputDelta("", "total 48\n")).toBe("total 48\n");
    expect(piToolOutputDelta("total 48\n", "total 48\nsrc/\n")).toBe("src/\n");
    expect(piToolOutputDelta("total 48\n", "total 48\n")).toBe("");
  });

  it("falls back to the whole text when the buffer is not a strict prefix", () => {
    expect(piToolOutputDelta("old output", "fresh output")).toBe("fresh output");
  });
});

describe("piToolItemType", () => {
  it("maps pi's built-ins onto canonical lifecycles", () => {
    expect(piToolItemType("bash")).toBe("command_execution");
    expect(piToolItemType("edit")).toBe("file_change");
    expect(piToolItemType("write")).toBe("file_change");
  });

  it("treats read-only built-ins and extension tools as generic tool calls", () => {
    expect(piToolItemType("read")).toBe("dynamic_tool_call");
    expect(piToolItemType("todowrite")).toBe("dynamic_tool_call");
    expect(piToolItemType(undefined)).toBe("dynamic_tool_call");
  });
});

describe("piContentStreamKind", () => {
  it("separates visible text from thinking", () => {
    expect(piContentStreamKind("text_delta")).toBe("assistant_text");
    expect(piContentStreamKind("thinking_delta")).toBe("reasoning_text");
  });

  it("ignores non-text deltas", () => {
    expect(piContentStreamKind("toolcall_delta")).toBeUndefined();
    expect(piContentStreamKind("")).toBeUndefined();
  });
});

describe("piTurnStateFromStopReason", () => {
  it("reports an abort as a cancellation", () => {
    expect(piTurnStateFromStopReason("aborted")).toBe("cancelled");
  });

  it("reports an error stop as a failure so the thread shows it", () => {
    expect(piTurnStateFromStopReason("error")).toBe("failed");
  });

  it("treats every other stop reason as a normal completion", () => {
    expect(piTurnStateFromStopReason("stop")).toBe("completed");
    expect(piTurnStateFromStopReason("length")).toBe("completed");
    expect(piTurnStateFromStopReason(undefined)).toBe("completed");
  });
});

describe("piContentBlocks", () => {
  it("wraps a bare string, which is how pi encodes simple user messages", () => {
    expect(piContentBlocks("hello")).toEqual([{ type: "text", text: "hello" }]);
  });

  it("keeps only well-formed blocks", () => {
    expect(
      piContentBlocks([
        { type: "text", text: "a" },
        { type: "thinking", thinking: "b" },
        { text: "no type" },
        null,
        "loose string",
      ]),
    ).toEqual([
      { type: "text", text: "a" },
      { type: "thinking", thinking: "b" },
    ]);
  });

  it("returns nothing for a non-array, non-string content", () => {
    expect(piContentBlocks(undefined)).toEqual([]);
    expect(piContentBlocks({ type: "text" })).toEqual([]);
  });
});

describe("piAssistantText", () => {
  it("concatenates visible text and leaves thinking out", () => {
    expect(
      piAssistantText([
        { type: "text", text: "Hello " },
        { type: "thinking", thinking: "hmm" },
        { type: "text", text: "world" },
      ]),
    ).toBe("Hello world");
  });
});

describe("piAssistantSegmentItemId", () => {
  it("keeps segment 0 on the historical assistant<turn> id so persisted ids stay stable", () => {
    expect(piAssistantSegmentItemId("turn-1", 0)).toBe("assistant-turn-1");
  });

  it("extends later messages with a segment suffix so they land on their own bubble", () => {
    expect(piAssistantSegmentItemId("turn-1", 1)).toBe("assistant-turn-1:seg:1");
    expect(piAssistantSegmentItemId("turn-1", 2)).toBe("assistant-turn-1:seg:2");
  });
});

describe("pi assistant message segments", () => {
  const makeSegments = () => new Map<string, PiTurnAssistantSegments>();

  it("opens segment 0 on the first delta and appends further text to it", () => {
    const segments = makeSegments();
    expect(piAppendAssistantDelta(segments, "turn-1", "Hello")).toEqual({
      segmentIndex: 0,
      text: "Hello",
    });
    expect(piAppendAssistantDelta(segments, "turn-1", " world")).toEqual({
      segmentIndex: 0,
      text: "Hello world",
    });
    expect(piOpenAssistantSegment(segments, "turn-1")?.text).toBe("Hello world");
  });

  it("closes the open message and starts the next one at segment 1", () => {
    const segments = makeSegments();
    piAppendAssistantDelta(segments, "turn-1", "First message");
    expect(piCloseAssistantSegment(segments, "turn-1")).toEqual({
      segmentIndex: 0,
      text: "First message",
    });
    expect(piCloseAssistantSegment(segments, "turn-1")).toBeUndefined();
    expect(piOpenAssistantSegment(segments, "turn-1")).toBeUndefined();

    expect(piAppendAssistantDelta(segments, "turn-1", "Second message")).toEqual({
      segmentIndex: 1,
      text: "Second message",
    });
    expect(piAssistantSegmentItemId("turn-1", 1)).toBe("assistant-turn-1:seg:1");
  });

  it("tracks turns independently so a steer on one does not disturb the other", () => {
    const segments = makeSegments();
    piAppendAssistantDelta(segments, "turn-1", "a");
    expect(piAppendAssistantDelta(segments, "turn-2", "b")).toEqual({
      segmentIndex: 0,
      text: "b",
    });
    expect(piOpenAssistantSegment(segments, "turn-1")?.text).toBe("a");
    expect(piOpenAssistantSegment(segments, "turn-2")?.text).toBe("b");
  });

  it("reports whether any text accumulated, for the agent_end recovery decision", () => {
    const segments = makeSegments();
    expect(piTurnHasAssistantText(segments, "turn-1")).toBe(false);
    piAppendAssistantDelta(segments, "turn-1", "streamed");
    expect(piTurnHasAssistantText(segments, "turn-1")).toBe(true);
    piCloseAssistantSegment(segments, "turn-1");
    expect(piTurnHasAssistantText(segments, "turn-1")).toBe(true);
  });

  it("ignores a close when no message is streaming", () => {
    const segments = makeSegments();
    expect(piCloseAssistantSegment(segments, "turn-1")).toBeUndefined();
    expect(piTurnHasAssistantText(segments, "turn-1")).toBe(false);
  });
});

describe("piUsageSnapshot", () => {
  it("sums the latest request's input, cache reads, and output", () => {
    expect(piUsageSnapshot({ input: 100, cacheRead: 900, output: 50 })).toEqual({
      usedTokens: 1050,
      inputTokens: 100,
      cachedInputTokens: 900,
      outputTokens: 50,
    });
  });

  it("omits fields pi did not report rather than defaulting them to zero", () => {
    expect(piUsageSnapshot({ output: 10 })).toEqual({ usedTokens: 10, outputTokens: 10 });
  });

  it("returns undefined for absent or empty usage so no bogus gauge is emitted", () => {
    expect(piUsageSnapshot(undefined)).toBeUndefined();
    expect(piUsageSnapshot({})).toBeUndefined();
    expect(piUsageSnapshot({ input: 0, output: 0 })).toBeUndefined();
  });
});
