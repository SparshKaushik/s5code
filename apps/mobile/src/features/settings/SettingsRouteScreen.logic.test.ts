import { describe, expect, it } from "vite-plus/test";

import { resolveAgentAwarenessPlatformPresentation } from "./SettingsRouteScreen.logic";

describe("resolveAgentAwarenessPlatformPresentation", () => {
  it("enables agent awareness settings on Android", () => {
    expect(resolveAgentAwarenessPlatformPresentation("android")).toEqual({
      supported: true,
      subtitle: undefined,
    });
  });

  it("enables agent awareness settings on iOS", () => {
    expect(resolveAgentAwarenessPlatformPresentation("ios")).toEqual({
      supported: true,
      subtitle: undefined,
    });
  });

  it("explains which native platforms support agent awareness settings", () => {
    expect(resolveAgentAwarenessPlatformPresentation("web")).toEqual({
      supported: false,
      subtitle: "Available on iOS and Android",
    });
  });
});
