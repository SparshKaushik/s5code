import type { UsageProviderKind } from "@t3tools/contracts";
import { useAppearancePreferences } from "../settings/appearance/AppearancePreferencesProvider";

/**
 * Series and table order. The chart stacks providers from the bottom in this
 * order, so it also fixes which band sits on top of the bars.
 */
export const PROVIDER_ORDER: readonly UsageProviderKind[] = [
  "codex",
  "claude",
  "cursor",
  "grok",
  "pi",
];

export const PROVIDER_LABEL: Record<UsageProviderKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  grok: "Grok Build",
  pi: "pi",
};

/**
 * Claude's brand orange holds in both themes; Codex and Grok are neutrals and
 * must flip with the theme or their bars vanish against the matching background.
 */
export function useProviderColors(): Record<UsageProviderKind, string> {
  const { themeAppearance: scheme } = useAppearancePreferences();
  return {
    claude: "#d97757",
    codex: scheme === "dark" ? "#e6e6e6" : "#3c3c43",
    cursor: "#8fa2b8",
    grok: scheme === "dark" ? "#a1a1aa" : "#52525b",
    pi: "#8b7cf6",
  };
}
