import * as NodeSqlite from "node:sqlite";

import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";

import {
  BUILD_MIGRATION_HEAD,
  databaseMigrationHead,
} from "../persistence/databaseMigrationHead.ts";
import { migrationManifest } from "../persistence/Migrations.ts";

import { runServicePreflight } from "./servicePreflight.ts";
import { SERVICE_LAUNCHER_PROTOCOL } from "./serviceProtocol.ts";

it.each([1, 2])("blocks legacy launcher protocol %i", (launcherProtocol) => {
  expect(
    runServicePreflight({
      databasePath: "/missing/state.sqlite",
      launcherProtocol,
      version: "1.2.3",
    }),
  ).toEqual({
    status: "blocked",
    version: "1.2.3",
    reason:
      "This release requires a newer S5 Code service launcher. Update it on the server machine.",
  });
});

it.layer(NodeServices.layer)("service preflight database", (it) => {
  it.effect("blocks a database that a newer build migrated", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const databasePath = path.join(
        yield* fs.makeTempDirectoryScoped({ prefix: "t3-preflight-" }),
        "state.sqlite",
      );
      const db = new NodeSqlite.DatabaseSync(databasePath);
      db.exec("CREATE TABLE effect_sql_migrations (migration_id INTEGER PRIMARY KEY, name TEXT)");
      const insert = db.prepare("INSERT INTO effect_sql_migrations VALUES (?, ?)");
      for (const [id, name] of migrationManifest) insert.run(id, name);
      const preflight = () =>
        runServicePreflight({ databasePath, launcherProtocol: SERVICE_LAUNCHER_PROTOCOL });

      expect(preflight().status).toBe("ready");
      expect(databaseMigrationHead(databasePath)).toBe(BUILD_MIGRATION_HEAD);

      const newerId = Math.max(...migrationManifest.map(([id]) => id)) + 1;
      insert.run(newerId, "FromNightly");
      db.close();
      const result = preflight();
      expect(result.status).toBe("blocked");
      expect(result.status === "blocked" ? result.reason : "").toContain(`${newerId}_FromNightly`);
      // Servers report this head, so clients see a database a newer build migrated.
      expect(databaseMigrationHead(databasePath)).toBe(newerId);

      // A database that exists but cannot be read has an unknown history, so it blocks.
      const unreadable = path.join(path.dirname(databasePath), "unreadable.sqlite");
      yield* fs.writeFileString(unreadable, "not a database");
      expect(databaseMigrationHead(unreadable)).toBeUndefined();
      expect(
        runServicePreflight({
          databasePath: unreadable,
          launcherProtocol: SERVICE_LAUNCHER_PROTOCOL,
        }).status,
      ).toBe("blocked");
      // A missing database is a fresh install.
      expect(databaseMigrationHead(path.join(path.dirname(databasePath), "missing.sqlite"))).toBe(
        BUILD_MIGRATION_HEAD,
      );
    }),
  );
});
