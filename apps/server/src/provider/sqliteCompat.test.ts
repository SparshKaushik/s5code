import { describe, expect, it } from "@effect/vitest";

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
