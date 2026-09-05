import type { CheckpointCleanupScope, CheckpointStorageUsage } from "@t3tools/contracts";

const BYTE_UNITS = ["KB", "MB", "GB", "TB"] as const;

/** Human-readable byte size. Mirrors the diagnostics panel's formatting. */
export function formatStorageBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  if (value < 1024) return `${Math.round(value)} B`;
  let unitIndex = -1;
  let next = value;
  do {
    next /= 1024;
    unitIndex += 1;
  } while (next >= 1024 && unitIndex < BYTE_UNITS.length - 1);
  return `${next.toFixed(next >= 10 ? 0 : 1)} ${BYTE_UNITS[unitIndex]}`;
}

export interface CheckpointUsageSummary {
  readonly totalBytesLabel: string;
  readonly orphanedBytesLabel: string;
  readonly entryCount: number;
  readonly orphanedCount: number;
  readonly refCount: number;
}

export function summarizeCheckpointUsage(usage: CheckpointStorageUsage): CheckpointUsageSummary {
  return {
    totalBytesLabel: formatStorageBytes(usage.totalBytes),
    orphanedBytesLabel: formatStorageBytes(usage.orphanedBytes),
    entryCount: usage.entries.length,
    orphanedCount: usage.entries.filter((entry) => entry.orphaned).length,
    refCount: usage.entries.reduce((total, entry) => total + entry.refCount, 0),
  };
}

/**
 * Copy for the cleanup buttons.
 *
 * `all` is deliberately blunt: it removes restore points for threads the user
 * still has, so the confirmation has to say that rather than talk about disk
 * space.
 */
export function describeCleanupScope(scope: CheckpointCleanupScope): {
  readonly label: string;
  readonly confirmTitle: string;
  readonly confirmDescription: string;
  readonly destructive: boolean;
} {
  switch (scope) {
    case "orphaned":
      return {
        label: "Clean up deleted threads",
        confirmTitle: "Remove checkpoint data for deleted threads?",
        confirmDescription:
          "Deletes hidden checkpoint commits that belong to threads you already deleted. Existing threads keep everything.",
        destructive: false,
      };
    case "retention-policy":
      return {
        label: "Apply retention policy",
        confirmTitle: "Apply the retention policy now?",
        confirmDescription:
          "Removes data for deleted threads, plus anything past the age or size limits below. Each existing thread keeps its most recent restore point.",
        destructive: false,
      };
    case "all":
      return {
        label: "Delete all checkpoint data",
        confirmTitle: "Delete every checkpoint commit?",
        confirmDescription:
          "Removes all hidden checkpoint commits, including for threads you are still working on. Those threads lose revert. Your branches, tags, commits, and working tree are untouched.",
        destructive: true,
      };
  }
}

export function formatCleanupResultDescription(input: {
  readonly removedEntryCount: number;
  readonly removedRefCount: number;
  readonly reclaimedBytes: number;
}): string {
  if (input.removedEntryCount === 0) {
    return "Nothing to clean up.";
  }
  const threads = `${input.removedEntryCount} ${input.removedEntryCount === 1 ? "entry" : "entries"}`;
  const refs =
    input.removedRefCount > 0
      ? ` and ${input.removedRefCount} ${input.removedRefCount === 1 ? "checkpoint" : "checkpoints"}`
      : "";
  return `Removed ${threads}${refs}, reclaiming ${formatStorageBytes(input.reclaimedBytes)}.`;
}
