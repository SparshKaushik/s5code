import { describe, expect, it } from "@effect/vitest";

import { isPiPlanToolName, piPlanStepsFromToolResult } from "./PiPlan.ts";

describe("isPiPlanToolName", () => {
  it("recognizes the todo tools regardless of case", () => {
    expect(isPiPlanToolName("todowrite")).toBe(true);
    expect(isPiPlanToolName("PatchTodo")).toBe(true);
    expect(isPiPlanToolName("read_todo")).toBe(true);
  });

  it("ignores unrelated tools", () => {
    expect(isPiPlanToolName("bash")).toBe(false);
    expect(isPiPlanToolName(undefined)).toBe(false);
  });
});

describe("piPlanStepsFromToolResult", () => {
  it("reads the structured details payload", () => {
    const steps = piPlanStepsFromToolResult({
      details: {
        todos: [
          { id: 1, content: "Read the adapter", status: "completed", priority: "high" },
          { id: 2, content: "Write the tests", status: "in_progress", priority: "high" },
          { id: 3, content: "Update docs", status: "pending", priority: "low" },
        ],
      },
    });
    expect(steps).toEqual([
      { step: "Read the adapter", status: "completed" },
      { step: "Write the tests", status: "inProgress" },
      { step: "Update docs", status: "pending" },
    ]);
  });

  it("reads a patchtodo result, which carries the whole list even though its args do not", () => {
    const steps = piPlanStepsFromToolResult({
      content: [{ type: "text", text: "Patched task 2" }],
      details: {
        patchedId: 2,
        todos: [
          { id: 1, content: "One", status: "completed" },
          { id: 2, content: "Two", status: "in_progress" },
        ],
      },
    });
    expect(steps).toHaveLength(2);
    expect(steps?.[1]).toEqual({ step: "Two", status: "inProgress" });
  });

  it("falls back to a JSON array in the text content", () => {
    const steps = piPlanStepsFromToolResult({
      content: [
        {
          type: "text",
          text: JSON.stringify([{ id: 1, content: "Only step", status: "pending" }]),
        },
      ],
    });
    expect(steps).toEqual([{ step: "Only step", status: "pending" }]);
  });

  it("treats unknown statuses as pending", () => {
    expect(
      piPlanStepsFromToolResult({ details: { todos: [{ content: "x", status: "blocked" }] } }),
    ).toEqual([{ step: "x", status: "pending" }]);
  });

  it("skips entries without content so a blank row never reaches the plan", () => {
    expect(
      piPlanStepsFromToolResult({
        details: { todos: [{ content: "   ", status: "pending" }, { status: "pending" }] },
      }),
    ).toEqual([]);
  });

  it("returns undefined when there is no todo payload, so the plan is left alone", () => {
    expect(
      piPlanStepsFromToolResult({ content: [{ type: "text", text: "done" }] }),
    ).toBeUndefined();
    expect(piPlanStepsFromToolResult(undefined)).toBeUndefined();
    expect(piPlanStepsFromToolResult("not an object")).toBeUndefined();
  });

  it("ignores malformed JSON in text content instead of failing", () => {
    expect(
      piPlanStepsFromToolResult({ content: [{ type: "text", text: "[{oops" }] }),
    ).toBeUndefined();
  });
});
