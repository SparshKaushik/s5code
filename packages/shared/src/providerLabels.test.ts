import { describe, expect, it } from "vite-plus/test";

import { resolveModelSubProvider } from "./providerLabels.ts";

describe("resolveModelSubProvider", () => {
  it("preserves labels discovered from OpenCode, including custom provider names", () => {
    expect(
      resolveModelSubProvider("opencode", {
        slug: "opencode/gpt-5",
        subProvider: "OpenCode (free)",
      }),
    ).toBe("OpenCode (free)");
  });

  it.each([
    ["openai/gpt-5", "OpenAI"],
    ["opencode/gpt-5", "OpenCode Zen"],
    ["opencode-go/gpt-5", "OpenCode Go"],
    ["antigravity/devin/claude-fable-5", "Antigravity"],
    ["github-copilot/claude-fable-5", "GitHub Copilot"],
    ["custom-provider/gpt-5", "Custom Provider"],
  ])("recovers the provider label from %s", (slug, label) => {
    expect(resolveModelSubProvider("opencode", { slug })).toBe(label);
  });

  it("does not mistake model paths from other drivers for OpenCode providers", () => {
    expect(resolveModelSubProvider("claudeAgent", { slug: "openai/gpt-5" })).toBeUndefined();
    expect(resolveModelSubProvider("opencode", { slug: "gpt-5" })).toBeUndefined();
    expect(resolveModelSubProvider("opencode", { slug: "/gpt-5" })).toBeUndefined();
    expect(resolveModelSubProvider("opencode", { slug: "openai/" })).toBeUndefined();
  });
});
