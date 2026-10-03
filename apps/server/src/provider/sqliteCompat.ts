// @effect-diagnostics nodeBuiltinImport:off -- SQLite's native backup API runs outside an Effect environment.
/**
 * Cross-runtime SQLite compatibility layer for Node.js and Bun.
 *
 * Node.js 22+ provides `DatabaseSync` via `node:sqlite`.
 * Bun provides `Database` via `bun:sqlite` and does not implement `node:sqlite`.
 *
 * This shim exports `DatabaseSync` compatible with both runtimes.
 *
 * @module provider/sqliteCompat
 */

import * as NodeFSP from "node:fs/promises";

interface DatabaseSyncOptions {
  readonly readOnly?: boolean | undefined;
  readonly timeout?: number | undefined;
  readonly allowExtension?: boolean | undefined;
  readonly enableForeignKeyConstraints?: boolean | undefined;
  readonly open?: boolean | undefined;
}

export class StatementSyncShim {
  private readonly query: any;
  private returnArrays = false;

  constructor(query: any) {
    this.query = query;
  }

  setReadBigInts(value: boolean): void {
    if (typeof this.query.safeIntegers === "function") {
      this.query.safeIntegers(Boolean(value));
    }
  }

  /**
   * Mirrors `node:sqlite`'s `StatementSync.columns()`. The Effect SQL client
   * uses the length to decide between `.all()` (row-returning) and `.run()`
   * (writes), so an absent method breaks every statement on Bun. Bun exposes
   * the result column names as `Query.columnNames`; the richer per-column
   * metadata Node reports is not available, so those fields are null.
   */
  columns(): ReadonlyArray<{
    column: string;
    database: string | null;
    name: string;
    table: string | null;
    type: string | null;
  }> {
    const names: ReadonlyArray<string> = this.query.columnNames ?? [];
    return names.map((name) => ({
      column: name,
      database: null,
      name,
      table: null,
      type: null,
    }));
  }

  setReturnArrays(value: boolean): void {
    this.returnArrays = Boolean(value);
  }

  all(...params: unknown[]): unknown[] {
    if (this.returnArrays && typeof this.query.values === "function") {
      return this.query.values(...params) ?? [];
    }
    return this.query.all(...params) ?? [];
  }

  run(...params: unknown[]): unknown {
    return this.query.run(...params);
  }

  get(...params: unknown[]): unknown {
    return this.query.get(...params);
  }

  /**
   * Lazily yields rows like `node:sqlite`'s `StatementSync.iterate`.
   * Bun's `Query` grew `iterate` in 1.2; older runtimes fall back to
   * materialising the row set.
   */
  *iterate(...params: unknown[]): IterableIterator<unknown> {
    if (typeof this.query.iterate === "function") {
      yield* this.query.iterate(...params);
      return;
    }
    yield* this.all(...params);
  }
}

class BunDatabaseSync {
  private readonly db: any;

  constructor(filename: string, options: DatabaseSyncOptions = {}) {
    const bunSqlite =
      (process as any).getBuiltinModule?.("bun:sqlite") ?? (globalThis as any).Bun?.sqlite;
    if (!bunSqlite?.Database) {
      throw new Error("bun:sqlite is not available in this environment");
    }
    const Database = bunSqlite.Database;
    this.db = new Database(filename, {
      readonly: Boolean(options.readOnly),
      create: !options.readOnly,
      readwrite: !options.readOnly,
    });
  }

  close(): void {
    this.db.close();
  }

  exec(sql: string): void {
    this.db.run(sql);
  }

  prepare(sql: string): StatementSyncShim {
    return new StatementSyncShim(this.db.query(sql));
  }

  loadExtension(path: string): void {
    if (typeof this.db.loadExtension === "function") {
      this.db.loadExtension(path);
    }
  }

  async backup(path: string): Promise<void> {
    // Bun serializes a consistent SQLite snapshot, including committed WAL
    // writes. V2's initial copy must not copy only the on-disk database file.
    await NodeFSP.writeFile(path, this.db.serialize());
  }
}

const nodeSqlite = (process as any).getBuiltinModule?.("node:sqlite");

export const DatabaseSync: any = nodeSqlite?.DatabaseSync ?? BunDatabaseSync;
export const StatementSync: any = nodeSqlite?.StatementSync ?? StatementSyncShim;
export const backup =
  nodeSqlite?.backup ?? ((database: BunDatabaseSync, path: string) => database.backup(path));
export default { DatabaseSync, StatementSync, backup };
