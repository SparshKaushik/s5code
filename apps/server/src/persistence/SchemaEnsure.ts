import * as Effect from "effect/Effect";
import * as SqlClient from "effect/unstable/sql/SqlClient";

const threadColumnNames = Effect.fn("schemaEnsure.threadColumnNames")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const columns = yield* sql<{ readonly name: string }>`
    PRAGMA table_info(projection_threads)
  `;
  return new Set(columns.map((column) => column.name));
});

const projectColumnNames = Effect.fn("schemaEnsure.projectColumnNames")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const columns = yield* sql<{ readonly name: string }>`
    PRAGMA table_info(projection_projects)
  `;
  return new Set(columns.map((column) => column.name));
});

const objectNames = Effect.fn("schemaEnsure.objectNames")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const rows = yield* sql<{ readonly name: string }>`
    SELECT name
    FROM sqlite_master
    WHERE type IN ('table', 'index')
  `;
  return new Set(rows.map((row) => row.name));
});

export const ensureExpectedSchema = Effect.fn("ensureExpectedSchema")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const healed: string[] = [];
  const threads = yield* threadColumnNames();
  const projects = yield* projectColumnNames();
  const objects = yield* objectNames();

  if (!threads.has("title_regeneration_request_id")) {
    yield* sql`
      ALTER TABLE projection_threads
      ADD COLUMN title_regeneration_request_id TEXT
    `;
    healed.push("projection_threads.title_regeneration_request_id");
  }

  if (!threads.has("title_regeneration_started_at")) {
    yield* sql`
      ALTER TABLE projection_threads
      ADD COLUMN title_regeneration_started_at TEXT
    `;
    healed.push("projection_threads.title_regeneration_started_at");
  }

  if (!threads.has("pinned_at")) {
    yield* sql`
      ALTER TABLE projection_threads
      ADD COLUMN pinned_at TEXT
    `;
    healed.push("projection_threads.pinned_at");
  }

  if (!threads.has("pin_order_key")) {
    yield* sql`
      ALTER TABLE projection_threads
      ADD COLUMN pin_order_key TEXT
    `;
    healed.push("projection_threads.pin_order_key");
  }

  if (!projects.has("default_thread_env_mode")) {
    yield* sql`
      ALTER TABLE projection_projects
      ADD COLUMN default_thread_env_mode TEXT
    `;
    healed.push("projection_projects.default_thread_env_mode");
  }

  if (!projects.has("favicon_path")) {
    yield* sql`
      ALTER TABLE projection_projects
      ADD COLUMN favicon_path TEXT
    `;
    healed.push("projection_projects.favicon_path");
  }

  if (objects.has("rewind_entries")) {
    yield* sql`
      DROP TABLE IF EXISTS rewind_entries
    `;
    healed.push("drop rewind_entries");
  }

  if (!objects.has("idx_projection_turns_thread_keyset")) {
    yield* sql`
      CREATE INDEX IF NOT EXISTS idx_projection_turns_thread_keyset
      ON projection_turns(thread_id, requested_at, turn_id)
    `;
    healed.push("idx_projection_turns_thread_keyset");
  }

  if (healed.length > 0) {
    yield* Effect.log("Healed missing database schema objects").pipe(
      Effect.annotateLogs({ objects: healed }),
    );
  }
});
