import type { UsageCatalogModelId, UsageModelAlias } from "@t3tools/contracts";
import { describe, expect, it } from "vite-plus/test";

import { applyTag, clearTag, type UsageModelTagTarget } from "./usageTags.ts";

const OPUS = "anthropic/claude-opus-5" as UsageCatalogModelId;
const SONNET = "anthropic/claude-sonnet-5" as UsageCatalogModelId;

function alias(overrides: Partial<UsageModelAlias> = {}): UsageModelAlias {
  return {
    provider: "pi",
    apiProvider: "agentrouter",
    model: "claude-opus-5",
    catalogModelId: OPUS,
    ...overrides,
  } as UsageModelAlias;
}

function untaggedTarget(apiProvider: string, model: string): UsageModelTagTarget {
  return { provider: "pi", label: model, untagged: { apiProvider, model }, taggedAs: null };
}

function taggedTarget(taggedAs: string): UsageModelTagTarget {
  return { provider: "pi", label: taggedAs, untagged: null, taggedAs };
}

describe("applyTag", () => {
  it("tags a model that had none", () => {
    const next = applyTag([], untaggedTarget("agentrouter", "claude-opus-5"), OPUS);
    expect(next).toEqual([
      {
        provider: "pi",
        apiProvider: "agentrouter",
        model: "claude-opus-5",
        catalogModelId: OPUS,
      },
    ]);
  });

  it("replaces a tag rather than adding a second one for the same model", () => {
    const next = applyTag([alias()], untaggedTarget("agentrouter", "claude-opus-5"), SONNET);
    expect(next).toHaveLength(1);
    expect(next[0]?.catalogModelId).toBe(SONNET);
  });

  it("leaves another gateway's tag for the same model name alone", () => {
    // Two resellers of one name are two products at two prices. Tagging one
    // must not silently retag the other.
    const next = applyTag(
      [alias(), alias({ apiProvider: "tokenrouter" })],
      untaggedTarget("agentrouter", "claude-opus-5"),
      SONNET,
    );
    expect(next).toHaveLength(2);
    expect(next.find((entry) => entry.apiProvider === "tokenrouter")?.catalogModelId).toBe(OPUS);
    expect(next.find((entry) => entry.apiProvider === "agentrouter")?.catalogModelId).toBe(SONNET);
  });

  it("retags every name feeding a consolidated row at once", () => {
    // The row the user is looking at is a merge of both names. Moving only one
    // would split it in two, which is not what "change this tag" means.
    const next = applyTag(
      [alias(), alias({ apiProvider: "tokenrouter", model: "opus-5-latest" })],
      taggedTarget(OPUS),
      SONNET,
    );
    expect(next).toHaveLength(2);
    expect(next.every((entry) => entry.catalogModelId === SONNET)).toBe(true);
  });

  it("does not touch tags belonging to another harness", () => {
    const claudeAlias = alias({ provider: "claude" });
    const next = applyTag([claudeAlias], untaggedTarget("agentrouter", "claude-opus-5"), SONNET);
    expect(next).toContainEqual(claudeAlias);
    expect(next).toHaveLength(2);
  });
});

describe("clearTag", () => {
  it("removes the tag for one untagged-row identity", () => {
    const kept = alias({ apiProvider: "tokenrouter" });
    expect(clearTag([alias(), kept], untaggedTarget("agentrouter", "claude-opus-5"))).toEqual([
      kept,
    ]);
  });

  it("removes every tag feeding a consolidated row", () => {
    const next = clearTag(
      [alias(), alias({ apiProvider: "tokenrouter", model: "opus-5-latest" })],
      taggedTarget(OPUS),
    );
    expect(next).toEqual([]);
  });

  it("keeps tags pointing at a different catalog entry", () => {
    const kept = alias({ model: "sonnet-5-latest", catalogModelId: SONNET });
    expect(clearTag([alias(), kept], taggedTarget(OPUS))).toEqual([kept]);
  });
});
