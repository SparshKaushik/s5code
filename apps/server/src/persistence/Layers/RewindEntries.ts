import { RewindFilePath } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";
import * as Struct from "effect/Struct";
import * as SqlClient from "effect/unstable/sql/SqlClient";
import * as SqlSchema from "effect/unstable/sql/SqlSchema";

import { toPersistenceDecodeError, toPersistenceSqlError } from "../Errors.ts";
import {
  DeleteRewindEntriesByStoreInput,
  RewindEntryRepository,
  RewindEntryRow,
  RewindStoreSummary,
  SetRewindEntryStateInput,
  ThreadRewindInput,
  type RewindEntryRepositoryShape,
} from "../Services/RewindEntries.ts";

const RewindEntryDbRowSchema = RewindEntryRow.mapFields(
  Struct.assign({
    files: Schema.fromJsonString(Schema.Array(RewindFilePath)),
  }),
);

function toPersistenceSqlOrDecodeError(sqlOperation: string, decodeOperation: string) {
  return (cause: unknown) =>
    Schema.isSchemaError(cause)
      ? toPersistenceDecodeError(decodeOperation)(cause)
      : toPersistenceSqlError(sqlOperation)(cause);
}

const makeRewindEntryRepository = Effect.gen(function* () {
  const sql = yield* SqlClient.SqlClient;

  const upsertRow = SqlSchema.void({
    Request: RewindEntryDbRowSchema,
    execute: (row) =>
      sql`
        INSERT INTO rewind_entries (
          thread_id,
          turn_id,
          sequence,
          store_id,
          cwd,
          user_message_id,
          assistant_message_id,
          prompt,
          before_tree,
          after_tree,
          files_json,
          state,
          created_at,
          updated_at
        )
        VALUES (
          ${row.threadId},
          ${row.turnId},
          ${row.sequence},
          ${row.storeId},
          ${row.cwd},
          ${row.userMessageId},
          ${row.assistantMessageId},
          ${row.prompt},
          ${row.beforeTree},
          ${row.afterTree},
          ${row.files},
          ${row.state},
          ${row.createdAt},
          ${row.updatedAt}
        )
        ON CONFLICT (thread_id, turn_id)
        DO UPDATE SET
          sequence = excluded.sequence,
          store_id = excluded.store_id,
          cwd = excluded.cwd,
          user_message_id = excluded.user_message_id,
          assistant_message_id = excluded.assistant_message_id,
          prompt = excluded.prompt,
          before_tree = excluded.before_tree,
          after_tree = excluded.after_tree,
          files_json = excluded.files_json,
          state = excluded.state,
          updated_at = excluded.updated_at
      `,
  });

  const listRowsByThread = SqlSchema.findAll({
    Request: ThreadRewindInput,
    Result: RewindEntryDbRowSchema,
    execute: ({ threadId }) =>
      sql`
        SELECT
          thread_id AS "threadId",
          turn_id AS "turnId",
          sequence,
          store_id AS "storeId",
          cwd,
          user_message_id AS "userMessageId",
          assistant_message_id AS "assistantMessageId",
          prompt,
          before_tree AS "beforeTree",
          after_tree AS "afterTree",
          files_json AS "files",
          state,
          created_at AS "createdAt",
          updated_at AS "updatedAt"
        FROM rewind_entries
        WHERE thread_id = ${threadId}
        ORDER BY sequence ASC
      `,
  });

  const findUndoCandidate = SqlSchema.findOneOption({
    Request: ThreadRewindInput,
    Result: RewindEntryDbRowSchema,
    execute: ({ threadId }) =>
      sql`
        SELECT
          thread_id AS "threadId",
          turn_id AS "turnId",
          sequence,
          store_id AS "storeId",
          cwd,
          user_message_id AS "userMessageId",
          assistant_message_id AS "assistantMessageId",
          prompt,
          before_tree AS "beforeTree",
          after_tree AS "afterTree",
          files_json AS "files",
          state,
          created_at AS "createdAt",
          updated_at AS "updatedAt"
        FROM rewind_entries
        WHERE thread_id = ${threadId}
          AND state = 'applied'
        ORDER BY sequence DESC
        LIMIT 1
      `,
  });

  const findRedoCandidate = SqlSchema.findOneOption({
    Request: ThreadRewindInput,
    Result: RewindEntryDbRowSchema,
    execute: ({ threadId }) =>
      sql`
        SELECT
          thread_id AS "threadId",
          turn_id AS "turnId",
          sequence,
          store_id AS "storeId",
          cwd,
          user_message_id AS "userMessageId",
          assistant_message_id AS "assistantMessageId",
          prompt,
          before_tree AS "beforeTree",
          after_tree AS "afterTree",
          files_json AS "files",
          state,
          created_at AS "createdAt",
          updated_at AS "updatedAt"
        FROM rewind_entries
        WHERE thread_id = ${threadId}
          AND state = 'undone'
        ORDER BY sequence ASC
        LIMIT 1
      `,
  });

  const updateState = SqlSchema.void({
    Request: SetRewindEntryStateInput,
    execute: ({ threadId, turnId, state, updatedAt }) =>
      sql`
        UPDATE rewind_entries
        SET state = ${state},
            updated_at = ${updatedAt}
        WHERE thread_id = ${threadId}
          AND turn_id = ${turnId}
      `,
  });

  const deleteRowsByThread = SqlSchema.void({
    Request: ThreadRewindInput,
    execute: ({ threadId }) =>
      sql`
        DELETE FROM rewind_entries
        WHERE thread_id = ${threadId}
      `,
  });

  const deleteUndoneRowsByThread = SqlSchema.void({
    Request: ThreadRewindInput,
    execute: ({ threadId }) =>
      sql`
        DELETE FROM rewind_entries
        WHERE thread_id = ${threadId}
          AND state = 'undone'
      `,
  });

  const deleteRowsByStore = SqlSchema.void({
    Request: DeleteRewindEntriesByStoreInput,
    execute: ({ storeId }) =>
      sql`
        DELETE FROM rewind_entries
        WHERE store_id = ${storeId}
      `,
  });

  const selectStoreSummaries = SqlSchema.findAll({
    Request: Schema.Struct({}),
    Result: RewindStoreSummary,
    execute: () =>
      sql`
        SELECT
          store_id AS "storeId",
          thread_id AS "threadId",
          MAX(cwd) AS "cwd",
          COUNT(*) AS "entryCount",
          MAX(updated_at) AS "updatedAt"
        FROM rewind_entries
        GROUP BY store_id, thread_id
      `,
  });

  const upsert: RewindEntryRepositoryShape["upsert"] = (row) =>
    upsertRow(row).pipe(
      Effect.mapError(
        toPersistenceSqlOrDecodeError(
          "RewindEntryRepository.upsert:query",
          "RewindEntryRepository.upsert:encodeRequest",
        ),
      ),
    );

  const listByThreadId: RewindEntryRepositoryShape["listByThreadId"] = (input) =>
    listRowsByThread(input).pipe(
      Effect.mapError(
        toPersistenceSqlOrDecodeError(
          "RewindEntryRepository.listByThreadId:query",
          "RewindEntryRepository.listByThreadId:decodeRows",
        ),
      ),
      Effect.map((rows) => rows as ReadonlyArray<RewindEntryRow>),
    );

  const getUndoCandidate: RewindEntryRepositoryShape["getUndoCandidate"] = (input) =>
    findUndoCandidate(input).pipe(
      Effect.mapError(
        toPersistenceSqlOrDecodeError(
          "RewindEntryRepository.getUndoCandidate:query",
          "RewindEntryRepository.getUndoCandidate:decodeRow",
        ),
      ),
      Effect.map(Option.map((row) => row as RewindEntryRow)),
    );

  const getRedoCandidate: RewindEntryRepositoryShape["getRedoCandidate"] = (input) =>
    findRedoCandidate(input).pipe(
      Effect.mapError(
        toPersistenceSqlOrDecodeError(
          "RewindEntryRepository.getRedoCandidate:query",
          "RewindEntryRepository.getRedoCandidate:decodeRow",
        ),
      ),
      Effect.map(Option.map((row) => row as RewindEntryRow)),
    );

  const setState: RewindEntryRepositoryShape["setState"] = (input) =>
    updateState(input).pipe(
      Effect.mapError(toPersistenceSqlError("RewindEntryRepository.setState:query")),
    );

  const deleteByThreadId: RewindEntryRepositoryShape["deleteByThreadId"] = (input) =>
    deleteRowsByThread(input).pipe(
      Effect.mapError(toPersistenceSqlError("RewindEntryRepository.deleteByThreadId:query")),
    );

  const deleteUndoneByThreadId: RewindEntryRepositoryShape["deleteUndoneByThreadId"] = (input) =>
    deleteUndoneRowsByThread(input).pipe(
      Effect.mapError(toPersistenceSqlError("RewindEntryRepository.deleteUndoneByThreadId:query")),
    );

  const deleteByStoreId: RewindEntryRepositoryShape["deleteByStoreId"] = (input) =>
    deleteRowsByStore(input).pipe(
      Effect.mapError(toPersistenceSqlError("RewindEntryRepository.deleteByStoreId:query")),
    );

  const listStoreSummaries: RewindEntryRepositoryShape["listStoreSummaries"] = () =>
    selectStoreSummaries({}).pipe(
      Effect.mapError(
        toPersistenceSqlOrDecodeError(
          "RewindEntryRepository.listStoreSummaries:query",
          "RewindEntryRepository.listStoreSummaries:decodeRows",
        ),
      ),
    );

  return {
    upsert,
    listByThreadId,
    getUndoCandidate,
    getRedoCandidate,
    setState,
    deleteUndoneByThreadId,
    deleteByThreadId,
    deleteByStoreId,
    listStoreSummaries,
  } satisfies RewindEntryRepositoryShape;
});

export const RewindEntryRepositoryLive = Layer.effect(
  RewindEntryRepository,
  makeRewindEntryRepository,
);
