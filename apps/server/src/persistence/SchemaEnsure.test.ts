import { assert, describe, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as SqlClient from "effect/unstable/sql/SqlClient";

import { runMigrations } from "./Migrations.ts";
import * as NodeSqliteClient from "@t3tools/shared/nodeSqliteClient";

const provideSqlite = <A, E, R>(effect: Effect.Effect<A, E, R>) =>
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

describe("SchemaEnsure and Migration Recovery", () => {
  it.effect(
    "recovers legacy fork migrations (38=RewindEntries, 39..41 shifted) to match upstream 38..40 and drops rewind table",
    () =>
      provideSqlite(
        Effect.gen(function* () {
          const sql = yield* SqlClient.SqlClient;

          yield* runMigrations({ toMigrationInclusive: 37 });
          yield* sql`
            CREATE TABLE rewind_entries (
              thread_id TEXT NOT NULL,
              turn_id TEXT NOT NULL,
              PRIMARY KEY (thread_id, turn_id)
            )
          `;
          yield* sql`
            INSERT INTO effect_sql_migrations (migration_id, name)
            VALUES
              (38, 'RewindEntries'),
              (39, 'ProjectionThreadsPinOrderKey'),
              (40, 'ProjectionProjectsDefaultThreadEnvMode'),
              (41, 'ProjectionProjectFaviconPath')
          `;

          yield* runMigrations();

          const migrations = yield* sql<{ readonly migration_id: number; readonly name: string }>`
            SELECT migration_id, name FROM effect_sql_migrations WHERE migration_id BETWEEN 38 AND 40 ORDER BY migration_id ASC
          `;

          assert.deepEqual(migrations, [
            { migration_id: 38, name: "ProjectionThreadsPinOrderKey" },
            { migration_id: 39, name: "ProjectionProjectsDefaultThreadEnvMode" },
            { migration_id: 40, name: "ProjectionProjectFaviconPath" },
          ]);

          const objects = yield* sqliteObjectNames();
          assert.ok(!objects.has("rewind_entries"));
        }),
      ),
  );

  it.effect(
    "recovers ID-35 RewindEntries layout without corrupting existing upstream IDs 38..40",
    () =>
      provideSqlite(
        Effect.gen(function* () {
          const sql = yield* SqlClient.SqlClient;

          yield* runMigrations({ toMigrationInclusive: 34 });
          yield* sql`
            INSERT INTO effect_sql_migrations (migration_id, name)
            VALUES
              (35, 'RewindEntries'),
              (36, 'ProjectionThreadsPinned'),
              (37, 'ProjectionTurnsKeysetIndex'),
              (38, 'ProjectionThreadsPinOrderKey'),
              (39, 'ProjectionProjectsDefaultThreadEnvMode'),
              (40, 'ProjectionProjectFaviconPath')
          `;

          yield* runMigrations();

          const migrations = yield* sql<{ readonly migration_id: number; readonly name: string }>`
            SELECT migration_id, name FROM effect_sql_migrations WHERE migration_id BETWEEN 35 AND 40 ORDER BY migration_id ASC
          `;

          assert.deepEqual(migrations, [
            { migration_id: 36, name: "ProjectionThreadsPinned" },
            { migration_id: 37, name: "ProjectionTurnsKeysetIndex" },
            { migration_id: 38, name: "ProjectionThreadsPinOrderKey" },
            { migration_id: 39, name: "ProjectionProjectsDefaultThreadEnvMode" },
            { migration_id: 40, name: "ProjectionProjectFaviconPath" },
          ]);

          const objects = yield* sqliteObjectNames();
          assert.ok(!objects.has("rewind_entries"));
        }),
      ),
  );

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
        assert.ok(!objects.has("rewind_entries"));
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
