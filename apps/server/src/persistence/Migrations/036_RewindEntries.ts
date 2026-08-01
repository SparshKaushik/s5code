import * as SqlClient from "effect/unstable/sql/SqlClient";
import * as Effect from "effect/Effect";

/**
 * Session rewind history (experimental).
 *
 * One row per captured turn. `before_tree` / `after_tree` are git tree oids
 * inside the thread's shadow store (`store_id`), never in the user's repo.
 * `state` tracks whether the turn's file changes are currently applied.
 */
export default Effect.gen(function* () {
  const sql = yield* SqlClient.SqlClient;

  yield* sql`
    CREATE TABLE IF NOT EXISTS rewind_entries (
      thread_id TEXT NOT NULL,
      turn_id TEXT NOT NULL,
      sequence INTEGER NOT NULL,
      store_id TEXT NOT NULL,
      cwd TEXT NOT NULL,
      user_message_id TEXT,
      assistant_message_id TEXT,
      prompt TEXT NOT NULL,
      before_tree TEXT NOT NULL,
      after_tree TEXT NOT NULL,
      files_json TEXT NOT NULL,
      state TEXT NOT NULL,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      PRIMARY KEY (thread_id, turn_id)
    )
  `;

  yield* sql`
    CREATE INDEX IF NOT EXISTS idx_rewind_entries_thread_sequence
    ON rewind_entries(thread_id, sequence)
  `;

  yield* sql`
    CREATE INDEX IF NOT EXISTS idx_rewind_entries_store
    ON rewind_entries(store_id)
  `;
});
