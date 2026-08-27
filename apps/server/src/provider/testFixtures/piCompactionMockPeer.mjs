// Deterministic `pi --mode rpc` peer for PiAdapter lifecycle tests.
//
// The prompt sequence mirrors an overflow recovery run: the first low-level
// agent run ends, Pi compacts, then a continuation emits a tool call before the
// session-level `agent_settled` boundary.
import * as NodeReadline from "node:readline";

const write = (value) => {
  process.stdout.write(`${JSON.stringify(value)}\n`);
};

const response = (id, command, data) =>
  write({
    type: "response",
    ...(typeof id === "string" ? { id } : {}),
    command,
    success: true,
    ...(data === undefined ? {} : { data }),
  });

const assistant = (stopReason, text = "") => ({
  role: "assistant",
  content: text.length > 0 ? [{ type: "text", text }] : [],
  stopReason,
  usage: { input: 1_000, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 1_001 },
});

const emitCompactionContinuation = () => {
  const overflow = assistant("length");

  write({ type: "agent_start" });
  write({ type: "message_end", message: overflow });
  write({ type: "turn_end", message: overflow, toolResults: [] });
  write({ type: "agent_end", messages: [overflow], willRetry: false });
  write({ type: "compaction_start", reason: "overflow" });
  write({
    type: "compaction_end",
    reason: "overflow",
    result: { summary: "compacted" },
    aborted: false,
    willRetry: true,
  });
  write({ type: "agent_start" });
  write({
    type: "tool_execution_start",
    toolCallId: "tool-after-compaction",
    toolName: "read",
    args: { path: "README.md" },
  });
  setTimeout(emitCompactionContinuationEnd, 20);
};

const emitCompactionContinuationEnd = () => {
  const completed = assistant("stop", "finished after compaction");
  write({
    type: "tool_execution_end",
    toolCallId: "tool-after-compaction",
    toolName: "read",
    result: { content: [{ type: "text", text: "contents" }], details: {} },
    isError: false,
  });
  write({
    type: "message_update",
    assistantMessageEvent: { type: "text_delta", delta: "finished after compaction" },
  });
  write({ type: "message_end", message: completed });
  write({ type: "turn_end", message: completed, toolResults: [] });
  write({ type: "agent_end", messages: [completed], willRetry: false });
  write({ type: "agent_settled" });
};

const lines = NodeReadline.createInterface({ input: process.stdin });
lines.on("line", (line) => {
  const command = JSON.parse(line);
  switch (command.type) {
    case "get_state":
      response(command.id, "get_state", {
        sessionId: "pi-adapter-mock-session",
        isStreaming: false,
        isCompacting: false,
        autoCompactionEnabled: true,
      });
      break;
    case "prompt":
      response(command.id, "prompt");
      setTimeout(emitCompactionContinuation, 25);
      break;
    case "abort":
      response(command.id, "abort");
      write({ type: "agent_settled" });
      break;
    default:
      response(command.id, String(command.type));
      break;
  }
});
