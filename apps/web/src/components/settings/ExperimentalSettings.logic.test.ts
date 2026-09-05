import { ThreadId, type CheckpointStorageEntry } from "@t3tools/contracts";
import { describe, expect, it } from "vite-plus/test";

import {
  describeCleanupScope,
  formatCleanupResultDescription,
  formatStorageBytes,
  summarizeCheckpointUsage,
} from "./ExperimentalSettings.logic";

const MEGABYTE = 1024 * 1024;

function entry(overrides: Partial<CheckpointStorageEntry> = {}): CheckpointStorageEntry {
  return {
    kind: "checkpoint-refs",
    threadId: ThreadId.make("thread-1"),
    label: "Thread 1",
    location: "/repo",
    refCount: 3,
    bytes: MEGABYTE,
    updatedAt: "2026-01-01T00:00:00.000Z",
    orphaned: false,
    ...overrides,
  };
}

describe("formatStorageBytes", () => {
  it("formats byte magnitudes", () => {
    expect(formatStorageBytes(0)).toBe("0 B");
    expect(formatStorageBytes(-5)).toBe("0 B");
    expect(formatStorageBytes(512)).toBe("512 B");
    expect(formatStorageBytes(2048)).toBe("2.0 KB");
    expect(formatStorageBytes(20 * 1024)).toBe("20 KB");
    expect(formatStorageBytes(3 * MEGABYTE)).toBe("3.0 MB");
    expect(formatStorageBytes(5 * 1024 * MEGABYTE)).toBe("5.0 GB");
  });
});

describe("summarizeCheckpointUsage", () => {
  it("counts entries and refs", () => {
    const summary = summarizeCheckpointUsage({
      generatedAt: "2026-01-01T00:00:00.000Z",
      entries: [
        entry(),
        entry({ threadId: ThreadId.make("thread-2"), orphaned: true, refCount: 2 }),
      ],
      totalBytes: 2 * MEGABYTE,
      orphanedBytes: MEGABYTE,
    });

    expect(summary).toEqual({
      totalBytesLabel: "2.0 MB",
      orphanedBytesLabel: "1.0 MB",
      entryCount: 2,
      orphanedCount: 1,
      refCount: 5,
    });
  });
});

describe("describeCleanupScope", () => {
  it("marks only the all scope as destructive", () => {
    expect(describeCleanupScope("orphaned").destructive).toBe(false);
    expect(describeCleanupScope("retention-policy").destructive).toBe(false);
    expect(describeCleanupScope("all").destructive).toBe(true);
  });

  it("warns that the all scope affects existing threads", () => {
    const copy = describeCleanupScope("all");
    expect(copy.confirmDescription).toContain("still working on");
    // The copy must reassure that user git data is safe.
    expect(copy.confirmDescription).toContain("branches");
  });
});

describe("formatCleanupResultDescription", () => {
  it("reports a no-op plainly", () => {
    expect(
      formatCleanupResultDescription({
        removedEntryCount: 0,
        removedRefCount: 0,
        reclaimedBytes: 0,
      }),
    ).toBe("Nothing to clean up.");
  });

  it("summarizes removals with reclaimed space", () => {
    expect(
      formatCleanupResultDescription({
        removedEntryCount: 2,
        removedRefCount: 5,
        reclaimedBytes: 2 * MEGABYTE,
      }),
    ).toBe("Removed 2 entries and 5 checkpoints, reclaiming 2.0 MB.");
  });

  it("omits the checkpoint clause when no checkpoints were removed", () => {
    expect(
      formatCleanupResultDescription({
        removedEntryCount: 1,
        removedRefCount: 0,
        reclaimedBytes: 1024,
      }),
    ).toBe("Removed 1 entry, reclaiming 1.0 KB.");
  });
});
