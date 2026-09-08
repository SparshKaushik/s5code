import type { SelectProviderOptionDescriptor } from "@t3tools/contracts";
import { describe, expect, it } from "@effect/vitest";

import {
  formatVariantLabel,
  openCode2CapabilitiesForModel,
  parseModelVariants,
  titleCaseSlug,
} from "./OpenCode2Provider.ts";

function variantDescriptor(
  capabilities: ReturnType<typeof openCode2CapabilitiesForModel>,
): SelectProviderOptionDescriptor | undefined {
  return capabilities.optionDescriptors?.find(
    (candidate): candidate is SelectProviderOptionDescriptor =>
      candidate.id === "variant" && candidate.type === "select",
  );
}

function variantOptionIds(
  capabilities: ReturnType<typeof openCode2CapabilitiesForModel>,
): Array<string> {
  const descriptor = variantDescriptor(capabilities);
  if (!descriptor) return [];
  return descriptor.options.map((option) => option.id);
}

describe("openCode2CapabilitiesForModel", () => {
  it("projects the full reasoning-effort ladder from the OpenCode array-shaped variants", () => {
    const capabilities = openCode2CapabilitiesForModel({
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
    const capabilities = openCode2CapabilitiesForModel({
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
});
