import { describe, expect, it } from "@effect/vitest";

import * as Effect from "effect/Effect";

import {
  piApprovalRequestType,
  piApprovalToolName,
  piExtensionUiQuestions,
  piExtensionUiResponsePayload,
  piShouldReportCompaction,
  piShouldSettleTurnAfterPrompt,
} from "./PiAdapter.ts";
import type { PiExtensionUiRequest } from "../pi/PiRpcSchemas.ts";

const request = (
  overrides: Partial<PiExtensionUiRequest> & Pick<PiExtensionUiRequest, "id" | "method">,
): PiExtensionUiRequest => ({ type: "extension_ui_request", ...overrides });

describe("piExtensionUiQuestions", () => {
  it("turns a select dialog into a single-select question over pi's options", () => {
    const questions = piExtensionUiQuestions(
      request({
        id: "ui-1",
        method: "select",
        title: "Pick a branch",
        options: ["main", "develop"],
      }),
    );
    expect(questions).toHaveLength(1);
    expect(questions[0]?.id).toBe("ui-1");
    expect(questions[0]?.multiSelect).toBe(false);
    expect(questions[0]?.options.map((option) => option.label)).toEqual(["main", "develop"]);
  });

  it("drops blank options so no empty choice renders", () => {
    const questions = piExtensionUiQuestions(
      request({ id: "ui-2", method: "select", title: "T", options: ["a", "  ", ""] }),
    );
    expect(questions[0]?.options.map((option) => option.label)).toEqual(["a"]);
  });

  it("gives an input dialog a single continue affordance alongside the free-text field", () => {
    const questions = piExtensionUiQuestions(
      request({
        id: "ui-3",
        method: "input",
        title: "Commit message",
        placeholder: "Describe the change",
      }),
    );
    expect(questions[0]?.header).toBe("Commit message");
    expect(questions[0]?.question).toBe("Describe the change");
    expect(questions[0]?.options).toEqual([
      { label: "Continue", description: "Submit your answer" },
    ]);
  });

  it("falls back to a generic header when pi sends no title", () => {
    expect(piExtensionUiQuestions(request({ id: "ui-4", method: "select" }))[0]?.header).toBe("pi");
  });
});

describe("piExtensionUiResponsePayload", () => {
  it("answers confirm with a boolean, which is the shape pi discriminates on", () => {
    const confirm = request({ id: "ui-1", method: "confirm", title: "Proceed?" });
    expect(piExtensionUiResponsePayload(confirm, { "ui-1": "Yes" })).toEqual({ confirmed: true });
    expect(piExtensionUiResponsePayload(confirm, {})).toEqual({ confirmed: false });
  });

  it("answers select and input with a value", () => {
    const select = request({ id: "ui-2", method: "select", options: ["main"] });
    expect(piExtensionUiResponsePayload(select, { "ui-2": "main" })).toEqual({ value: "main" });
  });

  it("takes the first entry when the panel submits an array", () => {
    const select = request({ id: "ui-3", method: "select", options: ["a", "b"] });
    expect(piExtensionUiResponsePayload(select, { "ui-3": ["b", "a"] })).toEqual({ value: "b" });
  });

  it("cancels a value dialog with no answer rather than sending an empty string", () => {
    const input = request({ id: "ui-4", method: "input" });
    expect(piExtensionUiResponsePayload(input, {})).toEqual({ cancelled: true });
    expect(piExtensionUiResponsePayload(input, { "ui-4": "   " })).toEqual({ cancelled: true });
  });
});

describe("piApprovalToolName", () => {
  it("reads the tool out of the runtime-mode extension's confirm title", () => {
    expect(piApprovalToolName(request({ id: "1", method: "confirm", title: "Allow bash?" }))).toBe(
      "bash",
    );
  });

  it("returns undefined for confirms raised by other extensions, so they are never auto-approved", () => {
    expect(
      piApprovalToolName(request({ id: "1", method: "confirm", title: "Delete the branch?" })),
    ).toBeUndefined();
    expect(piApprovalToolName(request({ id: "1", method: "confirm" }))).toBeUndefined();
  });
});

describe("piApprovalRequestType", () => {
  it("classifies the gated built-ins so the approval card renders correctly", () => {
    expect(piApprovalRequestType("bash")).toBe("exec_command_approval");
    expect(piApprovalRequestType("edit")).toBe("file_change_approval");
    expect(piApprovalRequestType("write")).toBe("file_change_approval");
  });

  it("falls back to a generic tool call for unknown or absent tools", () => {
    expect(piApprovalRequestType("mystery")).toBe("dynamic_tool_call");
    expect(piApprovalRequestType(undefined)).toBe("dynamic_tool_call");
  });
});

describe("piShouldReportCompaction", () => {
  it("reports successful compactions", () => {
    expect(piShouldReportCompaction({ aborted: false })).toBe(true);
  });

  it("does not report cancelled or failed compactions as successes", () => {
    expect(piShouldReportCompaction({ aborted: true })).toBe(false);
    expect(
      piShouldReportCompaction({
        aborted: false,
        errorMessage: "Auto-compaction failed: Summarization failed: 401",
      }),
    ).toBe(false);
  });
});

describe("piShouldSettleTurnAfterPrompt", () => {
  const check = (input: {
    isSteer?: boolean;
    turnIsStillActive?: boolean;
    sawAgentStart?: boolean;
    hasPendingDialog?: boolean;
    agentRunActive?: boolean;
  }) =>
    Effect.gen(function* () {
      let probed = false;
      const decision = yield* piShouldSettleTurnAfterPrompt({
        isSteer: input.isSteer ?? false,
        turnIsStillActive: input.turnIsStillActive ?? true,
        sawAgentStart: input.sawAgentStart ?? false,
        hasPendingDialog: input.hasPendingDialog ?? false,
        agentRunActive: () =>
          Effect.sync(() => {
            probed = true;
            return input.agentRunActive ?? false;
          }),
      });
      return { decision, probed };
    });

  it.effect("settles an extension command that ran to completion without an agent run", () =>
    Effect.gen(function* () {
      // `/diff` with nothing to report: no agent_start, no dialog, pi idle.
      expect((yield* check({})).decision).toBe(true);
    }),
  );

  it.effect("leaves a real prompt alone, since its agent_end closes the turn", () =>
    Effect.gen(function* () {
      expect((yield* check({ sawAgentStart: true })).decision).toBe(false);
    }),
  );

  it.effect("leaves a turn alone while a dialog is still waiting on the user", () =>
    Effect.gen(function* () {
      // `/repos` blocked on ctx.ui.select: the response that resolves it settles.
      expect((yield* check({ hasPendingDialog: true })).decision).toBe(false);
    }),
  );

  it.effect("leaves a turn alone when pi still reports a live agent run", () =>
    Effect.gen(function* () {
      expect((yield* check({ agentRunActive: true })).decision).toBe(false);
    }),
  );

  it.effect("never settles a steer, which belongs to a turn it does not own", () =>
    Effect.gen(function* () {
      expect((yield* check({ isSteer: true })).decision).toBe(false);
    }),
  );

  it.effect("does nothing when the turn was already closed underneath us", () =>
    Effect.gen(function* () {
      expect((yield* check({ turnIsStillActive: false })).decision).toBe(false);
    }),
  );

  it.effect("skips the get_state round-trip when a local signal already decided it", () =>
    Effect.gen(function* () {
      expect((yield* check({ sawAgentStart: true })).probed).toBe(false);
      expect((yield* check({ hasPendingDialog: true })).probed).toBe(false);
      expect((yield* check({ isSteer: true })).probed).toBe(false);
      expect((yield* check({})).probed).toBe(true);
    }),
  );
});
