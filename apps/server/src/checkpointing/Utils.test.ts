import { ThreadId } from "@t3tools/contracts";
import { describe, expect, it } from "vite-plus/test";

import { CHECKPOINT_REFS_PREFIX, checkpointRefForThreadTurn, parseCheckpointRef } from "./Utils.ts";

describe("parseCheckpointRef", () => {
  it("round-trips refs produced by checkpointRefForThreadTurn", () => {
    const threadId = ThreadId.make("thread-with/slashes and spaces");
    const ref = checkpointRefForThreadTurn(threadId, 7);

    expect(parseCheckpointRef(ref)).toEqual({ threadId, turnCount: 7 });
  });

  it("rejects refs outside the T3 checkpoint namespace", () => {
    // The safety property that makes bulk deletion viable: nothing a user owns
    // can ever be parsed as a checkpoint.
    expect(parseCheckpointRef("refs/heads/main")).toBeUndefined();
    expect(parseCheckpointRef("refs/tags/v1.0.0")).toBeUndefined();
    expect(parseCheckpointRef("refs/remotes/origin/main")).toBeUndefined();
    expect(parseCheckpointRef("refs/stash")).toBeUndefined();
    expect(parseCheckpointRef("refs/t3/rewind/abcdef")).toBeUndefined();
  });

  it("rejects malformed refs inside the namespace", () => {
    const encoded = "dGhyZWFkLTE";
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}/${encoded}`)).toBeUndefined();
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}/${encoded}/turn`)).toBeUndefined();
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}/${encoded}/branch/2`)).toBeUndefined();
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}/${encoded}/turn/x`)).toBeUndefined();
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}/${encoded}/turn/-1`)).toBeUndefined();
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}/${encoded}/turn/1.5`)).toBeUndefined();
    expect(parseCheckpointRef(`${CHECKPOINT_REFS_PREFIX}//turn/1`)).toBeUndefined();
  });

  it("accepts turn zero, which is the pre-turn baseline", () => {
    const threadId = ThreadId.make("thread-baseline");

    expect(parseCheckpointRef(checkpointRefForThreadTurn(threadId, 0))).toEqual({
      threadId,
      turnCount: 0,
    });
  });
});
