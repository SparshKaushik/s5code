/**
 * PiModelSupport: model slug encoding and capability derivation for pi.
 *
 * pi identifies a model by a `(provider, modelId)` pair, and `set_model` takes
 * both. S5 Code's `ModelSelection.model` is a single slug, so we encode the
 * pair as `provider/modelId` and split on the *first* separator only. Several
 * pi providers ship ids that themselves contain a slash (`cline-pass/glm-5.2`).
 *
 * Thinking levels are per-model. Pi's `thinkingLevelMap` selectively maps
 * canonical levels to provider-specific values. Missing core levels use Pi's
 * defaults, `null` marks a level as unsupported, and `xhigh` and `max` require
 * an explicit mapping. Models without reasoning support omit the control.
 *
 * @module provider/pi/PiModelSupport
 */
import type { ModelCapabilities, ModelSelection, ServerProviderModel } from "@t3tools/contracts";
import { createModelCapabilities, getModelSelectionStringOptionValue } from "@t3tools/shared/model";

import { parsePiThinkingLevel, type PiModel, type PiThinkingLevel } from "./PiRpcSchemas.ts";

export const PI_THINKING_OPTION_ID = "thinking";

/** Pi's built-in reasoning levels, enabled unless a model explicitly disables one. */
const DEFAULT_THINKING_LEVELS: ReadonlyArray<PiThinkingLevel> = [
  "off",
  "minimal",
  "low",
  "medium",
  "high",
];

/** Pi exposes these extended levels only when a model maps them explicitly. */
const EXPLICIT_THINKING_LEVELS: ReadonlyArray<PiThinkingLevel> = ["xhigh", "max"];

const THINKING_LEVEL_LABELS: Record<PiThinkingLevel, string> = {
  off: "Off",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
};

const DEFAULT_THINKING_LEVEL: PiThinkingLevel = "medium";

export interface PiModelRef {
  readonly provider: string;
  readonly modelId: string;
}

/** `provider/modelId`, the slug shape stored in `ModelSelection.model`. */
export function piModelSlug(model: PiModelRef): string {
  return `${model.provider}/${model.modelId}`;
}

/**
 * Split a slug back into pi's `(provider, modelId)` pair. Returns `undefined`
 * for a slug without a separator: a bare model id is ambiguous across pi
 * providers, and guessing would silently switch the user to a different model.
 */
export function parsePiModelSlug(slug: string | undefined): PiModelRef | undefined {
  const trimmed = slug?.trim();
  if (!trimmed) return undefined;
  const separatorIndex = trimmed.indexOf("/");
  if (separatorIndex <= 0 || separatorIndex === trimmed.length - 1) {
    return undefined;
  }
  return {
    provider: trimmed.slice(0, separatorIndex),
    modelId: trimmed.slice(separatorIndex + 1),
  };
}

/** Thinking levels a reasoning model accepts, in Pi's canonical order. */
export function piModelThinkingLevels(model: PiModel): ReadonlyArray<PiThinkingLevel> {
  if (model.reasoning !== true) return [];

  const levels = [...DEFAULT_THINKING_LEVELS, ...EXPLICIT_THINKING_LEVELS];
  return levels.filter((level) => {
    const mapped = model.thinkingLevelMap?.[level];
    if (mapped === null) return false;
    return !EXPLICIT_THINKING_LEVELS.includes(level) || mapped !== undefined;
  });
}

export function piModelCapabilities(model: PiModel): ModelCapabilities {
  const levels = piModelThinkingLevels(model);
  if (levels.length === 0) {
    return createModelCapabilities({ optionDescriptors: [] });
  }
  const defaultLevel = levels.includes(DEFAULT_THINKING_LEVEL)
    ? DEFAULT_THINKING_LEVEL
    : (levels[levels.length - 1] as PiThinkingLevel);
  return createModelCapabilities({
    optionDescriptors: [
      {
        id: PI_THINKING_OPTION_ID,
        label: "Thinking",
        type: "select",
        options: levels.map((level) =>
          level === defaultLevel
            ? { id: level, label: THINKING_LEVEL_LABELS[level], isDefault: true as const }
            : { id: level, label: THINKING_LEVEL_LABELS[level] },
        ),
        currentValue: defaultLevel,
      },
    ],
  });
}

export function piServerProviderModel(model: PiModel): ServerProviderModel {
  const name = model.name?.trim();
  return {
    slug: piModelSlug({ provider: model.provider, modelId: model.id }),
    name: name && name.length > 0 ? name : model.id,
    subProvider: model.provider,
    isCustom: false,
    capabilities: piModelCapabilities(model),
  };
}

/**
 * Thinking level requested for a turn, if the selection names a valid one.
 * Invalid values are dropped rather than coerced: pi rejects an unsupported
 * level, and silently substituting one would misreport what ran.
 */
export function piThinkingLevelFromSelection(
  modelSelection: ModelSelection | undefined,
): PiThinkingLevel | undefined {
  return parsePiThinkingLevel(
    getModelSelectionStringOptionValue(modelSelection, PI_THINKING_OPTION_ID),
  );
}
