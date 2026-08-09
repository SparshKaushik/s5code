import type { UsageProviderKind } from "@t3tools/contracts";
import { useColorScheme } from "react-native";

export const PROVIDER_ORDER: readonly UsageProviderKind[] = ["codex", "claude", "cursor", "pi"];

export const PROVIDER_LABEL: Record<UsageProviderKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  pi: "pi",
};

export function useProviderColors(): Record<UsageProviderKind, string> {
  const scheme = useColorScheme();
  return {
    claude: "#d97757",
    codex: scheme === "dark" ? "#e6e6e6" : "#3c3c43",
    cursor: "#8fa2b8",
    pi: "#8b7cf6",
  };
}
