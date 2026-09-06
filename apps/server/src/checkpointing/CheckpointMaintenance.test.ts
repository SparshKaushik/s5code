import { ThreadId, type CheckpointStorageEntry } from "@t3tools/contracts";
import * as DateTime from "effect/DateTime";
import { describe, expect, it } from "vite-plus/test";

import { compareByAgeAscending, selectRetentionEvictions } from "./CheckpointMaintenance.ts";

const MEGABYTE = 1024 * 1024;
const DAY_MS = 24 * 60 * 60 * 1000;
const NOW_MS = Date.parse("2026-03-01T00:00:00.000Z");

function entry(overrides: Partial<CheckpointStorageEntry> = {}): CheckpointStorageEntry {
  return {
    kind: "checkpoint-refs",
    threadId: ThreadId.make("thread-1"),
    label: "Thread 1",
    location: "/repo",
    refCount: 2,
    bytes: MEGABYTE,
    updatedAt: "2026-03-01T00:00:00.000Z",
    orphaned: false,
    ...overrides,
  };
}

function isoDaysAgo(days: number): string {
  return DateTime.formatIso(DateTime.makeUnsafe(NOW_MS - days * DAY_MS));
}

describe("compareByAgeAscending", () => {
  it("orders oldest first and sorts unknown timestamps to the front", () => {
    const unknown = entry({ label: "unknown", updatedAt: null });
    const older = entry({ label: "older", updatedAt: isoDaysAgo(10) });
    const newer = entry({ label: "newer", updatedAt: isoDaysAgo(1) });

    expect([newer, unknown, older].toSorted(compareByAgeAscending)).toEqual([
      unknown,
      older,
      newer,
    ]);
  });
});

describe("selectRetentionEvictions", () => {
  it("always removes orphaned entries, even with every limit disabled", () => {
    const orphan = entry({ label: "deleted thread", orphaned: true });
    const live = entry({ label: "live thread" });

    expect(
      selectRetentionEvictions({
        entries: [orphan, live],
        policy: { maxAgeMs: null, maxTotalBytes: null },
        nowMs: NOW_MS,
      }),
    ).toEqual([orphan]);
  });

  it("keeps the newest entry for a live thread even when it is past the age limit", () => {
    // The invariant that makes automatic sweeps safe: an existing thread never
    // loses its last restore point to a retention rule.
    const threadId = ThreadId.make("thread-old");
    const older = entry({ threadId, label: "turn 1", updatedAt: isoDaysAgo(90) });
    const newest = entry({ threadId, label: "turn 2", updatedAt: isoDaysAgo(60) });

    expect(
      selectRetentionEvictions({
        entries: [older, newest],
        policy: { maxAgeMs: 30 * DAY_MS, maxTotalBytes: null },
        nowMs: NOW_MS,
      }),
    ).toEqual([older]);
  });

  it("evicts oldest-first until the size budget is met", () => {
    const oldest = entry({
      threadId: ThreadId.make("thread-a"),
      label: "a1",
      bytes: 4 * MEGABYTE,
      updatedAt: isoDaysAgo(9),
    });
    const middle = entry({
      threadId: ThreadId.make("thread-a"),
      label: "a2",
      bytes: 4 * MEGABYTE,
      updatedAt: isoDaysAgo(5),
    });
    const newestForThreadA = entry({
      threadId: ThreadId.make("thread-a"),
      label: "a3",
      bytes: 4 * MEGABYTE,
      updatedAt: isoDaysAgo(1),
    });

    const evicted = selectRetentionEvictions({
      entries: [newestForThreadA, middle, oldest],
      policy: { maxAgeMs: null, maxTotalBytes: 9 * MEGABYTE },
      nowMs: NOW_MS,
    });

    // 12MB total, 9MB budget: dropping the oldest 4MB is enough, and the
    // newest entry stays protected regardless.
    expect(evicted).toEqual([oldest]);
  });

  it("does not evict below the budget when only protected entries remain", () => {
    const single = entry({ bytes: 100 * MEGABYTE, updatedAt: isoDaysAgo(400) });

    expect(
      selectRetentionEvictions({
        entries: [single],
        policy: { maxAgeMs: DAY_MS, maxTotalBytes: MEGABYTE },
        nowMs: NOW_MS,
      }),
    ).toEqual([]);
  });

  it("counts orphan reclamation toward the size budget before evicting live entries", () => {
    const orphan = entry({
      threadId: ThreadId.make("thread-gone"),
      label: "gone",
      bytes: 8 * MEGABYTE,
      orphaned: true,
      updatedAt: isoDaysAgo(2),
    });
    const liveOld = entry({
      threadId: ThreadId.make("thread-live"),
      label: "live old",
      bytes: MEGABYTE,
      updatedAt: isoDaysAgo(3),
    });
    const liveNew = entry({
      threadId: ThreadId.make("thread-live"),
      label: "live new",
      bytes: MEGABYTE,
      updatedAt: isoDaysAgo(1),
    });

    expect(
      selectRetentionEvictions({
        entries: [orphan, liveOld, liveNew],
        policy: { maxAgeMs: null, maxTotalBytes: 4 * MEGABYTE },
        nowMs: NOW_MS,
      }),
    ).toEqual([orphan]);
  });

  it("returns entries in input order so reports stay stable", () => {
    const first = entry({ label: "first", orphaned: true });
    const second = entry({ label: "second", orphaned: true });

    expect(
      selectRetentionEvictions({
        entries: [first, second],
        policy: { maxAgeMs: null, maxTotalBytes: null },
        nowMs: NOW_MS,
      }).map((removed) => removed.label),
    ).toEqual(["first", "second"]);
  });
});
