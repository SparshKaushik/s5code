import * as Schema from "effect/Schema";
import { describe, expect, it } from "@effect/vitest";

import { adoptS5ServerSettingsKeys, PersistedClientSettings } from "./s5SettingsKeys.ts";

const decode = Schema.decodeUnknownSync(PersistedClientSettings);
const encode = Schema.encodeSync(PersistedClientSettings);

const tag = {
  provider: "pi",
  apiProvider: "kiro",
  model: "claude-opus-5",
  catalogModelId: "anthropic/claude-opus-5",
};

describe("S5 settings key renames", () => {
  it("reads server model mappings saved under the S5 key, without replacing upstream's", () => {
    const legacy = JSON.parse(
      adoptS5ServerSettingsKeys(JSON.stringify({ usageModelMappings: { preview: "released" } })),
    );
    expect(legacy).toEqual({ usageModelAliases: { preview: "released" } });

    const both = JSON.stringify({
      usageModelMappings: { preview: "old" },
      usageModelAliases: { preview: "new" },
    });
    expect(adoptS5ServerSettingsKeys(both)).toBe(both);
    expect(adoptS5ServerSettingsKeys("{ // comment\n}")).toBe("{ // comment\n}");
  });

  it("keeps client gateway tags saved under the old key and writes the new one", () => {
    const settings = decode({ usageModelAliases: [tag] });
    expect(settings.usageModelTags).toEqual([tag]);
    expect(decode({ usageModelTags: [] }).usageModelTags).toEqual([]);

    const encoded = encode(settings) as Record<string, unknown>;
    expect(encoded.usageModelTags).toEqual([tag]);
    expect(encoded).not.toHaveProperty("usageModelAliases");
  });
});
