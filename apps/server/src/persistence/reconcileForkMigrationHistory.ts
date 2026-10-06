import * as Effect from "effect/Effect";
import * as Migrator from "effect/sql/Migrator";
import * as SqlClient from "effect/sql/SqlClient";

/** Align released S5 migration ids with upstream before its V2 upgrade runs. */
export const reconcileForkMigrationHistory = Effect.fn("reconcileForkMigrationHistory")(
  function* () {
    const sql = yield* SqlClient.SqlClient;
    yield* sql.withTransaction(
      Effect.gen(function* () {
        const tables = yield* sql`
          SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'effect_sql_migrations'
        `;
        if (tables.length === 0) return;
        const rows = yield* sql<{ readonly migration_id: number; readonly name: string }>`
          SELECT migration_id, name FROM effect_sql_migrations
        `;
        const nameById = new Map(rows.map((row) => [row.migration_id, row.name]));
        const remap = Effect.fnUntraced(function* (from: number, to: number, name: string) {
          if (nameById.get(from) !== name) return;
          if (nameById.has(to)) {
            return yield* new Migrator.MigrationError({
              kind: "BadState",
              message: `Cannot reconcile S5 migration ${from}_${name}: id ${to} is occupied.`,
            });
          }
          yield* sql`
            UPDATE effect_sql_migrations SET migration_id = ${to}
            WHERE migration_id = ${from} AND name = ${name}
          `;
          nameById.delete(from);
          nameById.set(to, name);
        });

        if (rows.some((row) => row.name === "RewindEntries")) {
          yield* sql`DELETE FROM effect_sql_migrations WHERE name = 'RewindEntries'`;
          for (const row of rows) {
            if (row.name === "RewindEntries") nameById.delete(row.migration_id);
          }
          if (rows.some((row) => row.migration_id === 38 && row.name === "RewindEntries")) {
            yield* remap(39, 38, "ProjectionThreadsPinOrderKey");
            yield* remap(40, 39, "ProjectionProjectsDefaultThreadEnvMode");
            yield* remap(41, 40, "ProjectionProjectFaviconPath");
          }
          yield* sql`DROP TABLE IF EXISTS rewind_entries`;
        }

        // S5 inserted its OpenCode rename at 52, shifting upstream 52–54 to
        // 53–55. Release V2 uses 55, so leaving the fork ledger would skip it.
        if (nameById.get(52) === "MigrateOpenCode2ToOpenCode") {
          yield* sql`
            DELETE FROM effect_sql_migrations
            WHERE migration_id = 52 AND name = 'MigrateOpenCode2ToOpenCode'
          `;
          nameById.delete(52);
        }
        yield* remap(53, 52, "ProjectionThreadTitleState");
        yield* remap(54, 53, "PullRequestFilesViewed");
        yield* remap(55, 54, "ProjectionThreadsAutoSettleDisabledAt");
      }),
    );
  },
);
