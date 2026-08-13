import { assert, describe, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as SqlClient from "effect/unstable/sql/SqlClient";

import { runMigrations } from "./Migrations.ts";
import * as NodeSqliteClient from "./NodeSqliteClient.ts";

const provideSqlite = <A, E, R>(effect: Effect.Effect<A, E, R | SqlClient.SqlClient>) =>
  effect.pipe(Effect.provide(NodeSqliteClient.layerMemory()));

const threadColumnNames = Effect.fn("threadColumnNames")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const columns = yield* sql<{ readonly name: string }>`
    PRAGMA table_info(projection_threads)
  `;
  return new Set(columns.map((column) => column.name));
});

const projectColumnNames = Effect.fn("projectColumnNames")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const columns = yield* sql<{ readonly name: string }>`
    PRAGMA table_info(projection_projects)
  `;
  return new Set(columns.map((column) => column.name));
});

const sqliteObjectNames = Effect.fn("sqliteObjectNames")(function* () {
  const sql = yield* SqlClient.SqlClient;
  const rows = yield* sql<{ readonly name: string }>`
    SELECT name
    FROM sqlite_master
    WHERE type IN ('table', 'index')
  `;
  return new Set(rows.map((row) => row.name));
});

describe("SchemaEnsure", () => {
  it.effect(
    "heals title regeneration columns when migration 35 was recorded as RewindEntries",
    () =>
      provideSqlite(
        Effect.gen(function* () {
          const sql = yield* SqlClient.SqlClient;

          yield* runMigrations({ toMigrationInclusive: 34 });
          yield* sql`
          INSERT INTO effect_sql_migrations (migration_id, name)
          VALUES (35, 'RewindEntries')
        `;
          yield* runMigrations();

          const threads = yield* threadColumnNames();
          assert.ok(threads.has("title_regeneration_request_id"));
          assert.ok(threads.has("title_regeneration_started_at"));
          assert.ok(threads.has("pinned_at"));
          assert.ok(threads.has("pin_order_key"));
        }),
      ),
  );

  it.effect("heals missing objects when later migration ids are already recorded", () =>
    provideSqlite(
      Effect.gen(function* () {
        const sql = yield* SqlClient.SqlClient;

        yield* runMigrations({ toMigrationInclusive: 34 });
        yield* sql`
          INSERT INTO effect_sql_migrations (migration_id, name)
          VALUES
            (35, 'RewindEntries'),
            (36, 'RewindEntries'),
            (37, 'RewindEntries'),
            (38, 'RewindEntries'),
            (39, 'RewindEntries'),
            (40, 'RewindEntries'),
            (41, 'RewindEntries')
        `;
        yield* runMigrations();

        const threads = yield* threadColumnNames();
        const projects = yield* projectColumnNames();
        const objects = yield* sqliteObjectNames();

        assert.ok(threads.has("title_regeneration_request_id"));
        assert.ok(threads.has("title_regeneration_started_at"));
        assert.ok(threads.has("pinned_at"));
        assert.ok(threads.has("pin_order_key"));
        assert.ok(projects.has("default_thread_env_mode"));
        assert.ok(projects.has("favicon_path"));
        assert.ok(objects.has("rewind_entries"));
        assert.ok(objects.has("idx_rewind_entries_thread_sequence"));
        assert.ok(objects.has("idx_rewind_entries_store"));
        assert.ok(objects.has("idx_projection_turns_thread_keyset"));
      }),
    ),
  );

  it.effect("does not heal later schema during a partial migration run", () =>
    provideSqlite(
      Effect.gen(function* () {
        yield* runMigrations({ toMigrationInclusive: 34 });

        const threads = yield* threadColumnNames();
        assert.ok(!threads.has("title_regeneration_request_id"));
        assert.ok(!threads.has("pinned_at"));
      }),
    ),
  );
});
