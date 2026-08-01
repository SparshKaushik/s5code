import * as Encoding from "effect/Encoding";
import * as Result from "effect/Result";
import { CHECKPOINT_REFS_PREFIX, CheckpointRef, ProjectId, ThreadId } from "@t3tools/contracts";

export { CHECKPOINT_REFS_PREFIX };

export function checkpointRefForThreadTurn(threadId: ThreadId, turnCount: number): CheckpointRef {
  return CheckpointRef.make(
    `${CHECKPOINT_REFS_PREFIX}/${Encoding.encodeBase64Url(threadId)}/turn/${turnCount}`,
  );
}

export interface ParsedCheckpointRef {
  readonly threadId: ThreadId;
  readonly turnCount: number;
}

/**
 * Recover `{threadId, turnCount}` from a checkpoint ref.
 *
 * Maintenance needs this to decide which refs belong to threads that still
 * exist. Returns `undefined` for anything that is not a well-formed ref this
 * build produced, so unrecognized refs under the namespace are left alone
 * instead of being attributed to the wrong thread.
 */
export function parseCheckpointRef(checkpointRef: string): ParsedCheckpointRef | undefined {
  if (!checkpointRef.startsWith(`${CHECKPOINT_REFS_PREFIX}/`)) {
    return undefined;
  }
  const segments = checkpointRef.slice(CHECKPOINT_REFS_PREFIX.length + 1).split("/");
  if (segments.length !== 3 || segments[1] !== "turn") {
    return undefined;
  }
  const [encodedThreadId, , rawTurnCount] = segments;
  if (encodedThreadId === undefined || rawTurnCount === undefined) {
    return undefined;
  }
  const turnCount = Number(rawTurnCount);
  if (!Number.isInteger(turnCount) || turnCount < 0) {
    return undefined;
  }
  const decoded = Encoding.decodeBase64UrlString(encodedThreadId);
  if (Result.isFailure(decoded) || decoded.success.length === 0) {
    return undefined;
  }
  return { threadId: ThreadId.make(decoded.success), turnCount };
}

export function resolveThreadWorkspaceCwd(input: {
  readonly thread: {
    readonly projectId: ProjectId;
    readonly worktreePath: string | null;
  };
  readonly projects: ReadonlyArray<{
    readonly id: ProjectId;
    readonly workspaceRoot: string;
  }>;
}): string | undefined {
  const worktreeCwd = input.thread.worktreePath ?? undefined;
  if (worktreeCwd) {
    return worktreeCwd;
  }

  return input.projects.find((project) => project.id === input.thread.projectId)?.workspaceRoot;
}
