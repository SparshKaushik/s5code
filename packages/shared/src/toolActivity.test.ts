import { describe, expect, it } from "vite-plus/test";

import { deriveToolActivityPresentation } from "./toolActivity.ts";

describe("toolActivity", () => {
  it("normalizes command tools to a stable ran-command label", () => {
    expect(
      deriveToolActivityPresentation({
        itemType: "command_execution",
        title: "Terminal",
        detail: "Terminal",
        data: {
          command: "bun run lint",
        },
        fallbackSummary: "Terminal",
      }),
    ).toEqual({
      summary: "Ran command",
      detail: "bun run lint",
    });
  });

  it("uses structured file paths for read-file tools when available", () => {
    expect(
      deriveToolActivityPresentation({
        itemType: "dynamic_tool_call",
        title: "Read File",
        detail: "Read File",
        data: {
          kind: "read",
          locations: [{ path: "/tmp/app.ts" }],
        },
        fallbackSummary: "Read File",
      }),
    ).toEqual({
      summary: "Read file",
      detail: "/tmp/app.ts",
    });
  });

  it("drops duplicated generic read-file detail when no path is available", () => {
    expect(
      deriveToolActivityPresentation({
        itemType: "dynamic_tool_call",
        title: "Read File",
        detail: "Read File",
        data: {
          kind: "read",
          rawInput: {},
        },
        fallbackSummary: "Read File",
      }),
    ).toEqual({
      summary: "Read file",
    });
  });

  it("finds the edited file inside an ACP diff content block when locations are sparse", () => {
    expect(
      deriveToolActivityPresentation({
        itemType: "dynamic_tool_call",
        title: "Edit file",
        detail: "Edit file",
        data: {
          kind: "edit",
          rawInput: {},
          content: [{ type: "diff", path: "/tmp/app.ts", oldText: "a", newText: "b" }],
        },
        fallbackSummary: "Edit file",
      }),
    ).toEqual({
      summary: "Changed files",
      detail: "/tmp/app.ts",
    });
  });

  it("finds the read file inside an ACP resource block uri when locations are sparse", () => {
    expect(
      deriveToolActivityPresentation({
        itemType: "dynamic_tool_call",
        title: "Read",
        detail: "Read",
        data: {
          kind: "read",
          rawInput: {},
          content: [
            {
              type: "content",
              content: {
                type: "resource",
                resource: {
                  textResourceContents: {
                    uri: "file:///tmp/app.ts",
                    mimeType: "text/plain",
                    text: "const x = 1;",
                  },
                },
              },
            },
          ],
        },
        fallbackSummary: "Read",
      }),
    ).toEqual({
      summary: "Read file",
      detail: "/tmp/app.ts",
    });
  });

  it("reads the command from rawOutput when Cursor omits rawInput", () => {
    expect(
      deriveToolActivityPresentation({
        itemType: "command_execution",
        title: "Terminal",
        detail: "Running checks",
        data: {
          kind: "execute",
          rawInput: {},
          rawOutput: { output: "bun run typecheck", command: "bun run typecheck" },
        },
        fallbackSummary: "Terminal",
      }),
    ).toEqual({
      summary: "Ran command",
      detail: "bun run typecheck",
    });
  });
});
