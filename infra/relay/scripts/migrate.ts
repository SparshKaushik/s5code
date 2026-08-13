/**
 * Apply pending relay Postgres migrations to the Neon database.
 *
 * The migrations under `../migrations/postgres` use Alchemy's snapshot format
 * (`{timestamp}_name/{migration.sql, snapshot.json}`), so they are applied with
 * Alchemy's own Postgres migration runner (`applyMigrations`), not drizzle-kit.
 * Each migration runs in a transaction together with bookkeeping in
 * `relay_migrations`, so a partially-failed migration cannot leave the schema
 * half-applied.
 *
 * `DATABASE_URL` (the Neon pooled or direct connection string) is read from the
 * environment. Run this out-of-band from the deploy, or from CI before
 * `vp run --filter t3code-relay deploy`:
 *
 *   vp run --filter t3code-relay migrate
 */
import { applyMigrations } from "alchemy/Neon/Migrations";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as Effect from "effect/Effect";
import * as Redacted from "effect/Redacted";

interface SqlFile {
  readonly id: string;
  readonly sql: string;
  readonly hash: string;
}

function numericPrefix(name: string): number | null {
  const prefix = name.split("_")[0];
  const value = Number.parseInt(prefix, 10);
  return Number.isNaN(value) ? null : value;
}

/**
 * Recursively list `.sql` files under `directory`, sorted by numeric prefix
 * (mirrors Alchemy's `listSqlFiles`, so migration ordering matches what the
 * original PlanetScale provider applied).
 */
function listSqlFiles(directory: string): SqlFile[] {
  const files: SqlFile[] = [];

  const walk = (relativeDir: string): void => {
    for (const entry of readdirSync(join(directory, relativeDir), { withFileTypes: true })) {
      const relativePath = relativeDir ? join(relativeDir, entry.name) : entry.name;
      if (entry.isDirectory()) {
        walk(relativePath);
      } else if (entry.name.endsWith(".sql")) {
        const sql = readFileSync(join(directory, relativePath), "utf8");
        files.push({
          id: relativePath,
          sql,
          hash: createHash("sha256").update(sql).digest("hex"),
        });
      }
    }
  };

  walk("");

  return files.sort((a, b) => {
    const aNum = numericPrefix(a.id);
    const bNum = numericPrefix(b.id);
    if (aNum !== null && bNum !== null) return aNum - bNum;
    if (aNum !== null) return -1;
    if (bNum !== null) return 1;
    return a.id.localeCompare(b.id);
  });
}

const migrationsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "migrations", "postgres");

const main = Effect.gen(function* () {
  const url = process.env.DATABASE_URL?.trim();
  if (!url) {
    return yield* Effect.die("DATABASE_URL is required to run relay migrations.");
  }

  const files = listSqlFiles(migrationsDir);
  if (files.length === 0) {
    yield* Effect.logWarning(`No migration files found under ${migrationsDir}`);
    return;
  }

  yield* applyMigrations({
    connectionUri: Redacted.make(url),
    migrationsTable: "relay_migrations",
    migrationsFiles: files,
  });

  yield* Effect.log(
    `Applied ${files.length} migration file(s) from ${migrationsDir} to ${files.at(-1)?.id ?? ""}`,
  );
});

Effect.runPromise(main).catch((cause) => {
  console.error(cause);
  process.exitCode = 1;
});
