/** Pi transcript usage, including gateway pricing and estimated Kiro context. */
import type { PiSettings } from "../settings.ts";
import {
  parseTimestampMs,
  tokenCount,
  totalTokens,
  type UsageRecord,
  type TranscriptUsageFormat,
  type ProviderUsageReader,
} from "@t3tools/provider-core/server/usage";
import type { UsageTokenTotals } from "@t3tools/contracts";
import { expandHomePath } from "@t3tools/provider-core/server/pathExpansion";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as Effect from "effect/Effect";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";

function finitePositive(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

export interface PiScanState {
  sessionId: string;
  /** Last estimated Kiro context by model, used for rolling-prefix cache simulation. */
  kiroContextTokensByModel: Record<string, number>;
}

export function initialPiScanState(): PiScanState {
  return { sessionId: "", kiroContextTokensByModel: {} };
}

/**
 * Feeds one line of a pi session log into `state`, returning a record when the
 * line was an assistant message with usage.
 *
 * pi's token fields are disjoint (`input + cacheRead + cacheWrite + output`
 * reconciles with its own `totalTokens`), so unlike Codex there is nothing to
 * subtract out. `reasoning` is a subset of `output`.
 *
 * pi also computes a cost per message, but only when it has rates for the
 * model; gateways it has no pricing for report a zero cost object next to real
 * tokens. A zero total is therefore read as "pi did not price this" and left to
 * our own pricing, which at worst agrees.
 */
export function parsePiLine(line: string, state: PiScanState): UsageRecord | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    return null;
  }
  return parsePiRecord(parsed, state);
}

function parsePiRecord(parsed: unknown, state: PiScanState): UsageRecord | null {
  if (typeof parsed !== "object" || parsed === null) return null;

  const record = parsed as Record<string, unknown>;

  if (record["type"] === "session") {
    if (typeof record["id"] === "string") state.sessionId = record["id"];
    return null;
  }
  if (record["type"] !== "message") return null;

  const message = record["message"];
  if (typeof message !== "object" || message === null) return null;
  const messageRecord = message as Record<string, unknown>;
  if (messageRecord["role"] !== "assistant") return null;

  const usage = messageRecord["usage"];
  if (typeof usage !== "object" || usage === null) return null;
  const usageRecord = usage as Record<string, unknown>;

  const timestampMs = parseTimestampMs(record["timestamp"]);
  if (timestampMs === null) return null;

  const model = typeof messageRecord["model"] === "string" ? messageRecord["model"] : "";
  if (model.length === 0) return null;

  const outputTokens = tokenCount(usageRecord["output"]);
  const apiProvider =
    typeof messageRecord["provider"] === "string" ? messageRecord["provider"] : "";
  const reportedInputTokens = tokenCount(usageRecord["input"]);
  const reportedCachedInputTokens = tokenCount(usageRecord["cacheRead"]);
  const reportedCacheCreationTokens = tokenCount(usageRecord["cacheWrite"]);
  const inputTokensEstimated =
    apiProvider === "kiro" &&
    reportedInputTokens > 0 &&
    reportedCachedInputTokens === 0 &&
    reportedCacheCreationTokens === 0;

  let uncachedInputTokens = reportedInputTokens;
  let cachedInputTokens = reportedCachedInputTokens;
  if (inputTokensEstimated) {
    // Kiro exposes context utilisation, not billed input/cache metrics. Its pi
    // adapter turns that percentage into `usage.input`, so treating every turn
    // as fresh input multiplies a long agent loop's apparent cost. Simulate a
    // conservative rolling-prefix cache: only the unchanged prefix of a
    // non-shrinking context is cached. A smaller context means compaction,
    // branch/reset, or a model switch and is treated as entirely fresh.
    const previousContextTokens = state.kiroContextTokensByModel[model] ?? 0;
    cachedInputTokens = reportedInputTokens >= previousContextTokens ? previousContextTokens : 0;
    uncachedInputTokens = reportedInputTokens - cachedInputTokens;
    state.kiroContextTokensByModel[model] = reportedInputTokens;
  }

  const totals: UsageTokenTotals = {
    uncachedInputTokens,
    cachedInputTokens,
    cacheCreationTokens: reportedCacheCreationTokens,
    outputTokens,
    reasoningTokens: Math.min(outputTokens, tokenCount(usageRecord["reasoning"])),
  };

  if (totalTokens(totals) === 0) return null;

  const cost = usageRecord["cost"];
  const reportedCostUsd =
    typeof cost === "object" && cost !== null
      ? finitePositive((cost as Record<string, unknown>)["total"])
      : null;

  const responseId =
    typeof messageRecord["responseId"] === "string" ? messageRecord["responseId"] : null;

  return {
    provider: "pi",
    timestampMs,
    model,
    apiProvider,
    sessionId: state.sessionId,
    totals,
    inputTokensEstimated,
    reportedCostUsd,
    speed: "standard",
    // The upstream response id survives a session fork, which copies messages
    // into a new file. pi's own message ids are short and only unique per file.
    dedupeKey: responseId,
  };
}

const orEmpty = (record: UsageRecord | null): readonly UsageRecord[] =>
  record === null ? [] : [record];
export const piUsageFormat: TranscriptUsageFormat<PiScanState> = {
  selectFields: {
    type: true,
    id: true,
    timestamp: true,
    message: { role: true, model: true, provider: true, usage: true, responseId: true },
  },
  mightCarryUsage: (line) => line.includes('"usage"') || line.includes('"session"'),
  parseLine: (line, state) => orEmpty(parsePiLine(line, state)),
  parseProjected: (projected, state) => orEmpty(parsePiRecord(projected, state)),
  state: {
    initial: initialPiScanState,
    schema: Schema.Struct({
      sessionId: Schema.String,
      kiroContextTokensByModel: Schema.Record(Schema.String, Schema.Number),
    }),
  },
};
export const piUsageReader: ProviderUsageReader<PiSettings, Path.Path> = {
  kind: "transcripts",
  provider: "pi",
  format: piUsageFormat,
  directories: Effect.fn("piUsageReader.directories")(function* ({ environment }) {
    const path = yield* Path.Path;
    const home = yield* HostProcess.HomeDirectory;
    const agentDir = environment.PI_CODING_AGENT_DIR?.trim() || path.join(home, ".pi", "agent");
    return [{ dir: path.resolve(expandHomePath(agentDir, home), "sessions") }];
  }),
};
