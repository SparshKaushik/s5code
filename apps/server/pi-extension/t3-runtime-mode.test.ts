import { describe, expect, it } from "@effect/vitest";

import { describeToolCall, parseRuntimeMode, requiresConfirmation } from "./t3-runtime-mode.ts";

describe("parseRuntimeMode", () => {
  it("accepts the three gated modes", () => {
    expect(parseRuntimeMode("approval-required")).toBe("approval-required");
    expect(parseRuntimeMode("auto-accept-edits")).toBe("auto-accept-edits");
    expect(parseRuntimeMode("auto")).toBe("auto");
  });

  it("falls back to full-access for anything unrecognized, so a bad env var cannot silently gate everything", () => {
    expect(parseRuntimeMode(undefined)).toBe("full-access");
    expect(parseRuntimeMode("")).toBe("full-access");
    expect(parseRuntimeMode("supervised")).toBe("full-access");
  });
});

describe("requiresConfirmation", () => {
  it("gates nothing in full-access", () => {
    expect(requiresConfirmation("full-access", "bash")).toBe(false);
    expect(requiresConfirmation("full-access", "edit")).toBe(false);
  });

  it("gates commands and edits when approval is required", () => {
    expect(requiresConfirmation("approval-required", "bash")).toBe(true);
    expect(requiresConfirmation("approval-required", "edit")).toBe(true);
    expect(requiresConfirmation("approval-required", "write")).toBe(true);
  });

  it("lets edits through but still asks for commands in auto-accept-edits", () => {
    expect(requiresConfirmation("auto-accept-edits", "edit")).toBe(false);
    expect(requiresConfirmation("auto-accept-edits", "write")).toBe(false);
    expect(requiresConfirmation("auto-accept-edits", "bash")).toBe(true);
  });

  it("treats auto like auto-accept-edits, since pi has no reviewer model", () => {
    expect(requiresConfirmation("auto", "edit")).toBe(false);
    expect(requiresConfirmation("auto", "bash")).toBe(true);
  });

  it("never gates read-only built-ins or extension tools", () => {
    for (const tool of ["read", "grep", "find", "ls", "todowrite", "some_custom_tool"]) {
      expect(requiresConfirmation("approval-required", tool)).toBe(false);
    }
  });
});

describe("describeToolCall", () => {
  it("shows the command for bash", () => {
    expect(describeToolCall({ toolName: "bash", input: { command: "rm -rf build" } })).toBe(
      "rm -rf build",
    );
  });

  it("falls back to a generic label when bash has no command", () => {
    expect(describeToolCall({ toolName: "bash", input: {} })).toBe("Run a shell command");
  });

  it("shows the target path for edits and writes", () => {
    expect(describeToolCall({ toolName: "edit", input: { path: "src/a.ts" } })).toBe(
      "Edit src/a.ts",
    );
    expect(describeToolCall({ toolName: "write", input: { path: "src/b.ts" } })).toBe(
      "Write src/b.ts",
    );
  });

  it("names the tool when there is nothing more specific to say", () => {
    expect(describeToolCall({ toolName: "mystery", input: {} })).toBe("Run mystery");
  });
});
