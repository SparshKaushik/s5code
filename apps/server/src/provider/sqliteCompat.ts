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

interface DatabaseSyncOptions {
  readonly readOnly?: boolean | undefined;
  readonly timeout?: number | undefined;
  readonly allowExtension?: boolean | undefined;
  readonly enableForeignKeyConstraints?: boolean | undefined;
  readonly open?: boolean | undefined;
}

class StatementSyncShim {
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
}

const nodeSqlite = (process as any).getBuiltinModule?.("node:sqlite");

export const DatabaseSync: any = nodeSqlite?.DatabaseSync ?? BunDatabaseSync;
export const StatementSync: any = nodeSqlite?.StatementSync ?? StatementSyncShim;
export default { DatabaseSync, StatementSync };
