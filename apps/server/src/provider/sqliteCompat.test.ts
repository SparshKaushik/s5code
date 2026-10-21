import * as NodeSqlite from "node:sqlite";

import { describe, expect, it } from "@effect/vitest";

import * as SqliteCompat from "./sqliteCompat.ts";
import { StatementSyncShim } from "./sqliteCompat.ts";

// Minimal stand-in for Bun's `bun:sqlite` Query. `NodeSqliteClient` calls
// `columns()` to decide between `.all()` and `.run()`, so the shim must answer
// it from `columnNames` — Bun's Query exposes names, not Node's richer metadata.
const query = (columnNames: ReadonlyArray<string>) => ({
  columnNames,
  all: () => [],
  run: () => ({}),
  values: () => [],
});

describe("StatementSyncShim", () => {
  it("reports one entry per result column", () => {
    const statement = new StatementSyncShim(query(["id", "name"]));

    expect(statement.columns()).toHaveLength(2);
    expect(statement.columns().map((column) => column.name)).toEqual(["id", "name"]);
  });

  it("reports no columns for statements that return no rows", () => {
    expect(new StatementSyncShim(query([])).columns()).toEqual([]);
  });

  it("degrades to no columns when the runtime exposes no column names", () => {
    expect(new StatementSyncShim({}).columns()).toEqual([]);
  });
});

describe("sqliteCompat on Node", () => {
  // The packaged server resolves `node:sqlite` to this module; a missing member
  // compiles to undefined and crashes the server at startup.
  it("exposes every node:sqlite member the SQLite client reads", () => {
    expect(SqliteCompat.DatabaseSync).toBe(NodeSqlite.DatabaseSync);
    expect(SqliteCompat.StatementSync).toBe(NodeSqlite.StatementSync);
    expect(SqliteCompat.backup).toBe(NodeSqlite.backup);
    expect(SqliteCompat.constants.SQLITE_TRANSACTION).toBe(NodeSqlite.constants.SQLITE_TRANSACTION);
  });
});
