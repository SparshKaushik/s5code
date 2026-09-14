import type { SelectProviderOptionDescriptor } from "@t3tools/contracts";
import { describe, expect, it } from "@effect/vitest";

import {
  formatVariantLabel,
  isOpenCodeVersionSupported,
  openCodeCapabilitiesForModel,
  parseModelVariants,
  titleCaseSlug,
} from "./OpenCodeProvider.ts";

function variantDescriptor(
  capabilities: ReturnType<typeof openCodeCapabilitiesForModel>,
): SelectProviderOptionDescriptor | undefined {
  return capabilities.optionDescriptors?.find(
    (candidate): candidate is SelectProviderOptionDescriptor =>
      candidate.id === "variant" && candidate.type === "select",
  );
}

function variantOptionIds(
  capabilities: ReturnType<typeof openCodeCapabilitiesForModel>,
): Array<string> {
  const descriptor = variantDescriptor(capabilities);
  if (!descriptor) return [];
  return descriptor.options.map((option) => option.id);
}

describe("openCodeCapabilitiesForModel", () => {
  it("projects the full reasoning-effort ladder from the OpenCode array-shaped variants", () => {
    const capabilities = openCodeCapabilitiesForModel({
      providerID: "kiro",
      variants: [
        { id: "none" },
        { id: "low" },
        { id: "medium" },
        { id: "high" },
        { id: "xhigh" },
        { id: "max" },
      ],
      agents: [{ name: "build", mode: "primary" }],
    });

    // Reflects the Kiro native effort ladder for GPT 5.6 (sol/terra/luna), which
    // includes `none` and `max` — previously dropped because variant `id` was
    // read as `name` and always came back undefined.
    expect(variantOptionIds(capabilities)).toEqual([
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);

    const descriptor = variantDescriptor(capabilities);
    expect(descriptor?.currentValue).toBe("high");
    expect(descriptor?.options.find((o) => o.id === "high")?.isDefault).toBe(true);
    expect(descriptor?.options.find((o) => o.id === "xhigh")?.label).toBe("Extra High");
    expect(descriptor?.options.find((o) => o.id === "none")?.label).toBe("None");
    expect(descriptor?.options.find((o) => o.id === "max")?.label).toBe("Max");
  });

  it("keeps the Claude ladder (low..max) without injecting `none`", () => {
    const capabilities = openCodeCapabilitiesForModel({
      providerID: "kiro",
      variants: [{ id: "low" }, { id: "medium" }, { id: "high" }, { id: "xhigh" }, { id: "max" }],
      agents: [{ name: "build", mode: "primary" }],
    });

    expect(variantOptionIds(capabilities)).toEqual(["low", "medium", "high", "xhigh", "max"]);
    const descriptor = variantDescriptor(capabilities);
    expect(descriptor?.currentValue).toBe("high");
    expect(descriptor?.options.find((o) => o.id === "high")?.isDefault).toBe(true);
  });

  it("formats labels correctly", () => {
    expect(formatVariantLabel("xhigh")).toBe("Extra High");
    expect(formatVariantLabel("none")).toBe("None");
    expect(formatVariantLabel("medium")).toBe("Medium");
    expect(formatVariantLabel("max")).toBe("Max");
  });

  it("formats OpenAI title case correctly", () => {
    expect(titleCaseSlug("openai")).toBe("OpenAI");
    expect(titleCaseSlug("kiro")).toBe("Kiro");
    expect(titleCaseSlug("google-vertex")).toBe("Google Vertex");
  });

  it("parses model variants from mixed formats", () => {
    expect(parseModelVariants(["none", "low", "high"])).toEqual([
      { id: "none" },
      { id: "low" },
      { id: "high" },
    ]);
    expect(parseModelVariants([{ id: "none" }, { id: "max" }])).toEqual([
      { id: "none" },
      { id: "max" },
    ]);
    expect(parseModelVariants(undefined)).toBeUndefined();
    expect(parseModelVariants([])).toBeUndefined();
  });

  it("normalizes agent options to lowercase identifiers", () => {
    const capabilities = openCodeCapabilitiesForModel({
      providerID: "opencode",
      agents: [
        { id: "build", name: "Build", mode: "primary" },
        { id: "plan", name: "Plan", mode: "primary" },
      ],
    });

    const agentDesc = capabilities.optionDescriptors?.find(
      (d): d is SelectProviderOptionDescriptor => d.id === "agent" && d.type === "select",
    );
    expect(agentDesc?.currentValue).toBe("build");
    expect(agentDesc?.options).toEqual([
      { id: "build", label: "Build", isDefault: true },
      { id: "plan", label: "Plan" },
    ]);
  });

  it("omits variant option descriptor when model has no variants", () => {
    const capabilities = openCodeCapabilitiesForModel({
      providerID: "hetzner",
      variants: [],
      agents: [{ id: "build", name: "Build", mode: "primary" }],
    });

    const variantDesc = capabilities.optionDescriptors?.find((d) => d.id === "variant");
    expect(variantDesc).toBeUndefined();
  });
});

describe("isOpenCodeVersionSupported", () => {
  it("accepts version 2.0.0 and above", () => {
    expect(isOpenCodeVersionSupported("2.0.0")).toBe(true);
    expect(isOpenCodeVersionSupported("2.1.0")).toBe(true);
    expect(isOpenCodeVersionSupported("v2.0.1")).toBe(true);
    expect(isOpenCodeVersionSupported("3.0.0")).toBe(true);
    expect(isOpenCodeVersionSupported("2.0.0-beta.1")).toBe(true);
  });

  it("accepts pre-release dev builds on version 0.0.0", () => {
    expect(isOpenCodeVersionSupported("0.0.0-beta-19425")).toBe(true);
    expect(isOpenCodeVersionSupported("0.0.0-dev-12345")).toBe(true);
  });

  it("rejects versions prior to 2.0.0", () => {
    expect(isOpenCodeVersionSupported("1.14.19")).toBe(false);
    expect(isOpenCodeVersionSupported("1.0.0")).toBe(false);
    expect(isOpenCodeVersionSupported("0.1.0")).toBe(false);
    expect(isOpenCodeVersionSupported(null)).toBe(false);
    expect(isOpenCodeVersionSupported(undefined)).toBe(false);
    expect(isOpenCodeVersionSupported("")).toBe(false);
    expect(isOpenCodeVersionSupported("invalid")).toBe(false);
  });
});
