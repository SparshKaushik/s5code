import type { UsageProviderKind } from "@t3tools/contracts";

import { ClaudeAI, CursorIcon, type Icon, OpenAI, PiAgentIcon } from "../Icons";

/**
 * Series and table order. The chart layers every provider from a shared zero
 * baseline, so this only fixes the reading order of legends, tables and hover
 * rows; it does not decide which series sits above the other.
 */
export const PROVIDER_ORDER: readonly UsageProviderKind[] = ["codex", "claude", "cursor", "pi"];

export const PROVIDER_LABEL: Record<UsageProviderKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  pi: "pi",
};

/** Claude's brand orange and pi's violet against a neutral white for Codex. */
export const PROVIDER_COLOR: Record<UsageProviderKind, string> = {
  claude: "#d97757",
  codex: "#e6e6e6",
  cursor: "#8fa2b8",
  pi: "#8b7cf6",
};

/**
 * Exhaustive presentation for providers supported by the usage contract.
 * Declaration order is reused by every chart, table, legend, and skeleton, so
 * adding a provider only requires its contract support and one entry here.
 */
export const PROVIDER_MARK: Record<UsageProviderKind, Icon> = {
  claude: ClaudeAI,
  codex: OpenAI,
  cursor: CursorIcon,
  pi: PiAgentIcon,
};

export const PROVIDER_PRESENTATION: Record<
  UsageProviderKind,
  { readonly label: string; readonly color: string; readonly mark: Icon }
> = Object.fromEntries(
  PROVIDER_ORDER.map((provider) => [
    provider,
    {
      label: PROVIDER_LABEL[provider],
      color: PROVIDER_COLOR[provider],
      mark: PROVIDER_MARK[provider],
    },
  ]),
) as Record<
  UsageProviderKind,
  { readonly label: string; readonly color: string; readonly mark: Icon }
>;
