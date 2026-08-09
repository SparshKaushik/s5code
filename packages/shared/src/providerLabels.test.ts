import { describe, expect, it } from "vite-plus/test";

import { formatProviderDriverName, formatProviderInstanceLabel } from "./providerLabels.ts";

describe("formatProviderDriverName", () => {
  it("names every shipped driver", () => {
    expect(formatProviderDriverName("codex")).toBe("Codex");
    expect(formatProviderDriverName("claudeAgent")).toBe("Claude");
    expect(formatProviderDriverName("cursor")).toBe("Cursor");
    expect(formatProviderDriverName("grok")).toBe("Grok");
    expect(formatProviderDriverName("opencode")).toBe("OpenCode");
  });

  it("keeps pi lowercase, since title-casing would misbrand it", () => {
    expect(formatProviderDriverName("pi")).toBe("pi");
  });

  it("still maps the pre-rename claude driver kind", () => {
    expect(formatProviderDriverName("claude")).toBe("Claude");
  });

  it("title-cases an unknown driver so a fork's driver still reads well", () => {
    expect(formatProviderDriverName("mycoder")).toBe("Mycoder");
    // The `Agent` suffix is an internal naming convention, not a display name.
    expect(formatProviderDriverName("someAgent")).toBe("Some");
  });

  it("falls back to a neutral label when there is no driver", () => {
    expect(formatProviderDriverName(null)).toBe("This agent");
    expect(formatProviderDriverName(undefined)).toBe("This agent");
    expect(formatProviderDriverName("")).toBe("This agent");
  });

  it("returns the original value when stripping the suffix leaves nothing", () => {
    expect(formatProviderDriverName("Agent")).toBe("Agent");
  });
});

describe("formatProviderInstanceLabel", () => {
  it("prefers a user-set display name", () => {
    expect(
      formatProviderInstanceLabel({ displayName: "Work pi", driver: "pi", instanceId: "pi-work" }),
    ).toBe("Work pi");
  });

  it("ignores a blank display name", () => {
    expect(
      formatProviderInstanceLabel({ displayName: "   ", driver: "pi", instanceId: "pi" }),
    ).toBe("pi");
  });

  it("falls back to the driver name", () => {
    expect(formatProviderInstanceLabel({ driver: "opencode", instanceId: "oc-2" })).toBe(
      "OpenCode",
    );
  });

  it("uses the instance id only when the driver is unnameable", () => {
    // Unique but not descriptive, so it is the last resort rather than a default.
    expect(formatProviderInstanceLabel({ driver: "", instanceId: "custom-1" })).toBe("custom-1");
  });
});
