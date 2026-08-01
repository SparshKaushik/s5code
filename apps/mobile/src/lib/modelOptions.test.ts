import { describe, expect, it } from "vite-plus/test";

import { ProviderInstanceId, type ServerConfig } from "@t3tools/contracts";

import { buildModelOptions } from "./modelOptions";

describe("mobile model options", () => {
  it("normalizes a legacy fallback selection against current capabilities", () => {
    const config = {
      providers: [
        {
          instanceId: "codex",
          driver: "codex",
          displayName: "Codex",
          enabled: true,
          installed: true,
          auth: { status: "authenticated" },
          models: [
            {
              slug: "gpt-test",
              name: "GPT Test",
              isCustom: false,
              capabilities: {
                optionDescriptors: [
                  {
                    id: "serviceTier",
                    label: "Service Tier",
                    type: "select",
                    options: [
                      { id: "default", label: "Standard", isDefault: true },
                      { id: "priority", label: "Fast" },
                    ],
                    currentValue: "default",
                  },
                ],
              },
            },
          ],
        },
      ],
    } as unknown as ServerConfig;

    const [option] = buildModelOptions(config, {
      instanceId: ProviderInstanceId.make("codex"),
      model: "gpt-test",
      options: [{ id: "fastMode", value: true }],
    });

    expect(option?.capabilities?.optionDescriptors?.[0]?.id).toBe("serviceTier");
    expect(option?.selection.options).toEqual([{ id: "serviceTier", value: "default" }]);
  });
});

describe("provider labels", () => {
  const providerConfig = (
    provider: Record<string, unknown>,
    models: ReadonlyArray<Record<string, unknown>>,
  ) =>
    ({
      providers: [
        {
          enabled: true,
          installed: true,
          auth: { status: "authenticated" },
          models,
          ...provider,
        },
      ],
    }) as unknown as ServerConfig;

  it("names pi in lowercase rather than falling back to the instance id", () => {
    const options = buildModelOptions(
      providerConfig({ instanceId: "pi", driver: "pi" }, [
        { slug: "kiro/claude-sonnet-5", name: "Claude Sonnet 5", subProvider: "kiro" },
      ]),
      null,
    );
    expect(options[0]?.providerLabel).toBe("pi");
  });

  it("uses driver names for every shipped driver", () => {
    const labelFor = (driver: string) =>
      buildModelOptions(
        providerConfig({ instanceId: `${driver}-instance`, driver }, [{ slug: "m", name: "M" }]),
        null,
      )[0]?.providerLabel;

    expect(labelFor("codex")).toBe("Codex");
    expect(labelFor("claudeAgent")).toBe("Claude");
    expect(labelFor("cursor")).toBe("Cursor");
    expect(labelFor("grok")).toBe("Grok");
    expect(labelFor("opencode")).toBe("OpenCode");
  });

  it("prefers a user-set display name over the driver name", () => {
    const options = buildModelOptions(
      providerConfig({ instanceId: "pi-work", driver: "pi", displayName: "Work pi" }, [
        { slug: "m", name: "M" },
      ]),
      null,
    );
    expect(options[0]?.providerLabel).toBe("Work pi");
  });

  it("qualifies aggregated models by vendor so duplicate names stay distinct", () => {
    // pi surfaces the same model through several vendors; unqualified rows
    // would read identically in the picker.
    const options = buildModelOptions(
      providerConfig({ instanceId: "pi", driver: "pi" }, [
        { slug: "kiro/claude-sonnet-5", name: "Claude Sonnet 5", subProvider: "kiro" },
        { slug: "anthropic/claude-sonnet-5", name: "Claude Sonnet 5", subProvider: "anthropic" },
      ]),
      null,
    );
    expect(options.map((option) => option.label)).toEqual([
      "kiro · Claude Sonnet 5",
      "anthropic · Claude Sonnet 5",
    ]);
  });

  it("leaves a model alone when its name already leads with the vendor", () => {
    const options = buildModelOptions(
      providerConfig({ instanceId: "pi", driver: "pi" }, [
        { slug: "xai/grok-4.5", name: "Grok 4.5", subProvider: "xai" },
        { slug: "openai/gpt-5", name: "OpenAI GPT-5", subProvider: "openai" },
      ]),
      null,
    );
    expect(options.map((option) => option.label)).toEqual(["xai · Grok 4.5", "OpenAI GPT-5"]);
  });

  it("does not qualify single-vendor providers, which have no ambiguity", () => {
    const options = buildModelOptions(
      providerConfig({ instanceId: "codex", driver: "codex" }, [
        { slug: "gpt-5.6", name: "GPT-5.6" },
      ]),
      null,
    );
    expect(options[0]?.label).toBe("GPT-5.6");
  });
});
