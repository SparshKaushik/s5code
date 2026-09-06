import type { UsageProviderKind } from "@t3tools/contracts";

import { ClaudeAI, CursorIcon, GrokIcon, type Icon, OpenAI, PiAgentIcon } from "../Icons";

type UsageProviderPresentation = {
  readonly label: string;
  readonly color: string;
  readonly mark: Icon;
};

export const PROVIDER_PRESENTATION = {
  codex: {
    label: "Codex",
    color: "var(--contrast-foreground)",
    mark: OpenAI,
  },
  claude: {
    label: "Claude Code",
    color: "#d97757",
    mark: ClaudeAI,
  },
  cursor: {
    label: "Cursor",
    color: "#8fa2b8",
    mark: CursorIcon,
  },
  grok: {
    label: "Grok Build",
    color: "color-mix(in oklab, var(--contrast-foreground) 72%, var(--background))",
    mark: GrokIcon,
  },
  pi: {
    label: "pi",
    color: "#8b7cf6",
    mark: PiAgentIcon,
  },
} satisfies Record<UsageProviderKind, UsageProviderPresentation>;

/** Stable provider reading order across charts, summaries, tables, and hover rows. */
export const PROVIDER_ORDER = Object.keys(PROVIDER_PRESENTATION) as UsageProviderKind[];

export const PROVIDER_LABEL: Record<UsageProviderKind, string> = {
  codex: PROVIDER_PRESENTATION.codex.label,
  claude: PROVIDER_PRESENTATION.claude.label,
  cursor: PROVIDER_PRESENTATION.cursor.label,
  grok: PROVIDER_PRESENTATION.grok.label,
  pi: PROVIDER_PRESENTATION.pi.label,
};

export const PROVIDER_COLOR: Record<UsageProviderKind, string> = {
  codex: PROVIDER_PRESENTATION.codex.color,
  claude: PROVIDER_PRESENTATION.claude.color,
  cursor: PROVIDER_PRESENTATION.cursor.color,
  grok: PROVIDER_PRESENTATION.grok.color,
  pi: PROVIDER_PRESENTATION.pi.color,
};

export const PROVIDER_MARK: Record<UsageProviderKind, Icon> = {
  codex: PROVIDER_PRESENTATION.codex.mark,
  claude: PROVIDER_PRESENTATION.claude.mark,
  cursor: PROVIDER_PRESENTATION.cursor.mark,
  grok: PROVIDER_PRESENTATION.grok.mark,
  pi: PROVIDER_PRESENTATION.pi.mark,
};

/** Providers with real activity, independent of the metric currently displayed. */
export function providersWithUsage(
  totals: readonly {
    readonly provider: UsageProviderKind;
    readonly costUsd: number;
    readonly totalTokens: number;
  }[],
): readonly UsageProviderKind[] {
  const active = new Set(
    totals
      .filter((entry) => entry.totalTokens > 0 || entry.costUsd > 0)
      .map((entry) => entry.provider),
  );
  return PROVIDER_ORDER.filter((provider) => active.has(provider));
}
