import { describe, expect, it } from "@effect/vitest";
import { ProviderInstanceId, type ModelSelection } from "@t3tools/contracts";

import {
  parsePiModelSlug,
  piModelCapabilities,
  piModelThinkingLevels,
  piServerProviderModel,
  piModelSlug,
  piThinkingLevelFromSelection,
  PI_THINKING_OPTION_ID,
} from "./PiModelSupport.ts";
import type { PiModel } from "./PiRpcSchemas.ts";

const model = (overrides: Partial<PiModel> & Pick<PiModel, "id" | "provider">): PiModel => ({
  ...overrides,
});

describe("parsePiModelSlug", () => {
  it("splits on the first separator so provider-qualified ids survive", () => {
    expect(parsePiModelSlug("clinepass/cline-pass/glm-5.2")).toEqual({
      provider: "clinepass",
      modelId: "cline-pass/glm-5.2",
    });
  });

  it("round-trips through piModelSlug", () => {
    const ref = { provider: "kiro", modelId: "claude-opus-5" };
    expect(parsePiModelSlug(piModelSlug(ref))).toEqual(ref);
  });

  it("rejects a bare model id rather than guessing a provider", () => {
    expect(parsePiModelSlug("claude-opus-5")).toBeUndefined();
    expect(parsePiModelSlug("")).toBeUndefined();
    expect(parsePiModelSlug(undefined)).toBeUndefined();
  });

  it("rejects slugs missing either side of the separator", () => {
    expect(parsePiModelSlug("/claude-opus-5")).toBeUndefined();
    expect(parsePiModelSlug("kiro/")).toBeUndefined();
  });
});

describe("piModelThinkingLevels", () => {
  it("uses Pi defaults for unmapped core levels", () => {
    expect(
      piModelThinkingLevels(
        model({
          id: "claude-opus-5",
          provider: "kiro",
          reasoning: true,
          thinkingLevelMap: { off: "off", low: "low", high: "high" },
        }),
      ),
    ).toEqual(["off", "minimal", "low", "medium", "high"]);
  });

  it("removes null-mapped levels and ignores unknown map keys", () => {
    expect(
      piModelThinkingLevels(
        model({
          id: "m",
          provider: "p",
          reasoning: true,
          thinkingLevelMap: { off: null, bogus: "x" },
        }),
      ),
    ).toEqual(["minimal", "low", "medium", "high"]);
  });

  it("offers extended levels only when Pi maps them explicitly", () => {
    expect(
      piModelThinkingLevels(
        model({
          id: "m",
          provider: "p",
          reasoning: true,
          thinkingLevelMap: { xhigh: "xhigh", max: null },
        }),
      ),
    ).toEqual(["off", "minimal", "low", "medium", "high", "xhigh"]);
  });

  it("offers no levels for a non-reasoning model", () => {
    expect(
      piModelThinkingLevels(
        model({ id: "gpt-5-6-sol", provider: "kiro", thinkingLevelMap: { low: "low" } }),
      ),
    ).toEqual([]);
  });

  it("uses Pi's default ladder for a reasoning model with no map", () => {
    expect(
      piModelThinkingLevels(model({ id: "grok-4.3", provider: "xai", reasoning: true })),
    ).toEqual(["off", "minimal", "low", "medium", "high"]);
  });
});

describe("piModelCapabilities", () => {
  it("omits the thinking control when the model takes no levels", () => {
    const capabilities = piModelCapabilities(model({ id: "gpt-5-6-sol", provider: "kiro" }));
    expect(capabilities.optionDescriptors).toEqual([]);
  });

  it("offers Gemini 3.7 Flash minimal through high when Pi disables Off", () => {
    const capabilities = piModelCapabilities(
      model({
        id: "gemini-3.7-flash",
        provider: "google-vertex",
        reasoning: true,
        thinkingLevelMap: { off: null },
      }),
    );
    const descriptor = capabilities.optionDescriptors?.[0];

    expect(descriptor).toMatchObject({
      id: PI_THINKING_OPTION_ID,
      type: "select",
      currentValue: "medium",
    });
    if (descriptor?.type !== "select") {
      throw new Error("Expected Gemini thinking to be a select option");
    }
    expect(descriptor.options).toEqual([
      { id: "minimal", label: "Minimal" },
      { id: "low", label: "Low" },
      { id: "medium", label: "Medium", isDefault: true },
      { id: "high", label: "High" },
    ]);
  });

  it("defaults to medium when the model supports it", () => {
    const capabilities = piModelCapabilities(
      model({
        id: "claude-opus-5",
        provider: "kiro",
        reasoning: true,
        thinkingLevelMap: { off: "off", low: "low", medium: "medium", high: "high" },
      }),
    );
    const descriptor = capabilities.optionDescriptors?.[0];
    expect(descriptor?.id).toBe(PI_THINKING_OPTION_ID);
    expect(descriptor?.type).toBe("select");
    expect(descriptor?.currentValue).toBe("medium");
  });

  it("falls back to the highest offered level when medium is unavailable", () => {
    const capabilities = piModelCapabilities(
      model({
        id: "m",
        provider: "p",
        reasoning: true,
        thinkingLevelMap: { off: "off", medium: null, high: null },
      }),
    );
    expect(capabilities.optionDescriptors?.[0]?.currentValue).toBe("low");
  });
});

describe("piServerProviderModel", () => {
  it("qualifies the slug and keeps the pi provider as the sub-provider", () => {
    const entry = piServerProviderModel(
      model({ id: "cline-pass/glm-5.2", provider: "clinepass", name: "GLM-5.2 (ClinePass)" }),
    );
    expect(entry.slug).toBe("clinepass/cline-pass/glm-5.2");
    expect(entry.name).toBe("GLM-5.2 (ClinePass)");
    expect(entry.subProvider).toBe("clinepass");
    expect(entry.isCustom).toBe(false);
  });

  it("falls back to the id when pi reports no display name", () => {
    expect(piServerProviderModel(model({ id: "grok-4.3", provider: "xai" })).name).toBe("grok-4.3");
  });
});

describe("piThinkingLevelFromSelection", () => {
  const selection = (value: string): ModelSelection => ({
    instanceId: ProviderInstanceId.make("pi"),
    model: "kiro/claude-opus-5",
    options: [{ id: PI_THINKING_OPTION_ID, value }],
  });

  it("reads a valid level", () => {
    expect(piThinkingLevelFromSelection(selection("high"))).toBe("high");
  });

  it("drops an unsupported level instead of coercing it", () => {
    expect(piThinkingLevelFromSelection(selection("ludicrous"))).toBeUndefined();
  });

  it("returns undefined when no selection is present", () => {
    expect(piThinkingLevelFromSelection(undefined)).toBeUndefined();
  });
});
