import * as NodeSqlite from "node:sqlite";

export function openCursorAuthDatabase(databasePath: string) {
  return new NodeSqlite.DatabaseSync(databasePath, { readOnly: true });
}
