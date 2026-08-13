/**
 * Apply pending relay Postgres migrations to the Neon database.
 *
 * The migrations under `../migrations/postgres` use Alchemy's snapshot format
 * (`{timestamp}_name/{migration.sql, snapshot.json}`), so they are applied
 * directly with `pg` (not drizzle-kit). Each migration runs in a transaction
 * together with bookkeeping in `relay_migrations`, so a partially-failed
 * migration cannot leave the schema half-applied.
 *
 * `DATABASE_URL` (the Neon pooled or direct connection string) is read from the
 * environment. Run out-of-band, or from CI before the deploy:
 *
 *   vp run --filter t3code-relay migrate
 */
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { Client } = require("pg");

/**
 * Strip query-string SSL flags from a Neon connection URI. Neon's URIs include
 * `sslmode=require` and `channel_binding=require`; we control SSL via the `ssl`
 * option below instead, so remove the conflicting query params.
 */
function stripSslQueryParams(uri) {
  try {
    const url = new URL(uri);
    url.searchParams.delete("sslmode");
    url.searchParams.delete("channel_binding");
    return url.toString();
  } catch {
    return uri;
  }
}

function numericPrefix(name) {
  const prefix = name.split("_")[0];
  const value = Number.parseInt(prefix, 10);
  return Number.isNaN(value) ? null : value;
}

/**
 * Recursively list `.sql` files under `directory`, sorted by numeric prefix
 * (mirrors Alchemy's `listSqlFiles`, so ordering matches the original
 * PlanetScale provider).
 */
function listSqlFiles(directory) {
  const files = [];

  const walk = (relativeDir) => {
    for (const entry of readdirSync(join(directory, relativeDir), { withFileTypes: true })) {
      const relativePath = relativeDir ? join(relativeDir, entry.name) : entry.name;
      if (entry.isDirectory()) {
        walk(relativePath);
      } else if (entry.name.endsWith(".sql")) {
        const sql = readFileSync(join(directory, relativePath), "utf8");
        // Match the existing `relay_migrations.name` convention: the migration
        // directory basename (e.g. `20260527044716_baseline`), not the full
        // relative `.../migration.sql` path. Idempotency relies on this.
        const id = relativeDir || entry.name.replace(/\.sql$/, "");
        files.push({
          id,
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

async function applyMigrations(connectionUri, migrationsTable, files) {
  const client = new Client({
    connectionString: stripSslQueryParams(connectionUri),
    ssl: { rejectUnauthorized: false },
  });
  await client.connect();
  try {
    await client.query(
      `CREATE TABLE IF NOT EXISTS "${migrationsTable}" (
         id TEXT PRIMARY KEY,
         name TEXT NOT NULL,
         applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
       );`,
    );

    const appliedRows = await client.query(`SELECT name FROM "${migrationsTable}";`);
    const applied = new Set(appliedRows.rows.map((row) => row.name));

    let nextSeq = 0;
    for (const { id } of await (
      await client.query(`SELECT id FROM "${migrationsTable}";`)
    ).rows) {
      if (/^\d+$/.test(id)) nextSeq = Math.max(nextSeq, Number.parseInt(id, 10));
    }
    nextSeq += 1;

    let appliedCount = 0;
    for (const file of files) {
      if (applied.has(file.id)) continue;
      const migrationId = nextSeq.toString().padStart(5, "0");
      nextSeq += 1;
      await client.query("BEGIN");
      try {
        await client.query(file.sql);
        await client.query(`INSERT INTO "${migrationsTable}" (id, name) VALUES ($1, $2);`, [
          migrationId,
          file.id,
        ]);
        await client.query("COMMIT");
        appliedCount += 1;
      } catch (error) {
        await client.query("ROLLBACK").catch(() => {});
        throw error;
      }
    }
    return appliedCount;
  } finally {
    await client.end().catch(() => {});
  }
}

const migrationsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "migrations", "postgres");

async function main() {
  const url = process.env.DATABASE_URL?.trim();
  if (!url) {
    throw new Error("DATABASE_URL is required to run relay migrations.");
  }

  const files = listSqlFiles(migrationsDir);
  if (files.length === 0) {
    console.warn(`No migration files found under ${migrationsDir}`);
    return;
  }

  const appliedCount = await applyMigrations(url, "relay_migrations", files);
  console.log(
    `Applied ${appliedCount} migration(s), ${files.length - appliedCount} already present (${migrationsDir}).`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
