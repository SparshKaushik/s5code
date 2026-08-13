/**
 * `t3 migrate-data` - one-time migration of an existing T3 Code data directory
 * into the S5 Code home (`.s5code`).
 *
 * S5 Code rebranded the app bundle and the on-disk home directory: the storage
 * root moved from `~/.t3` to `~/.s5code` (see `os-jank.ts` / `DesktopStatePaths.ts`).
 * Existing installs keep their data under `~/.t3`. This command copies that data
 * (the SQLite database, its WAL/SHM siblings, plus `secrets` and `settings.json`)
 * into `~/.s5code`. It does NOT apply migrations itself: the S5 server runs its
 * migration set automatically on the next startup (`MigrationsLive` in
 * `persistence/Migrations.ts`), so the copied database lands at the S5 schema
 * version after the first `t3 serve` / `t3 start`.
 *
 * Safety:
 * - Refuses to overwrite a non-empty target unless `--force` is passed.
 * - Copies the SQLite file plus its `-wal`/`-shm` siblings so a clean shutdown
 *   history survives intact.
 */
import * as Console from "effect/Console";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import { Command, Flag } from "effect/unstable/cli";

import { resolveBaseDir } from "../os-jank.ts";

// The legacy T3 home predates the `.s5code` rebrand, so its default is the
// historical `~/.t3` regardless of `resolveBaseDir`'s new `.s5code` default.
const resolveLegacyBaseDir = Effect.fn(function* (raw: string | undefined) {
  if (raw && raw.trim().length > 0) {
    return yield* resolveBaseDir(raw);
  }
  const { join } = yield* Path.Path;
  return join(process.env.HOME ?? process.cwd(), ".t3");
});

const SQLITE_FILENAMES = ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"];
const COPY_ENTRIES = ["secrets", "settings.json", "saved-environments.json"];

export class MigrateDataError extends Schema.TaggedErrorClass<MigrateDataError>()(
  "MigrateDataError",
  { message: Schema.String, suggestion: Schema.String },
) {}

const exists = (fs: FileSystem.FileSystem, inputPath: string) =>
  fs.exists(inputPath).pipe(Effect.orElseSucceed(() => false));

const copyFile = (fs: FileSystem.FileSystem, from: string, to: string) =>
  fs.copyFile(from, to).pipe(
    Effect.mapError(
      (error) =>
        new MigrateDataError({
          message: `Failed to copy ${from} to ${to}: ${error.message}`,
          suggestion: "Check file permissions and available disk space.",
        }),
    ),
  );

const joinStateEntries = (
  root: string,
  joinPath: Path.Path["join"],
  list: ReadonlyArray<string>,
): ReadonlyArray<{ from: string; to: string }> =>
  list.map((name) => ({ from: joinPath(root, "userdata", name), to: joinPath(root, name) }));

const migrateData = Effect.fn("cli.migrateData")(function* (input: {
  readonly sourceHome: string | undefined;
  readonly targetHome: string | undefined;
  readonly force: boolean;
}) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;

  const sourceBaseDir = yield* resolveLegacyBaseDir(input.sourceHome);
  const targetBaseDir = yield* resolveBaseDir(input.targetHome);

  const sourceStateDir = path.join(sourceBaseDir, "userdata");
  const targetStateDir = path.join(targetBaseDir, "userdata");

  if (sourceBaseDir === targetBaseDir) {
    return yield* new MigrateDataError({
      message: "Source and target data directory are the same.",
      suggestion: "Start the S5 Code server against the existing directory instead.",
    });
  }

  const sourceDbExists = yield* exists(fs, path.join(sourceStateDir, "state.sqlite"));
  if (!sourceDbExists) {
    return yield* new MigrateDataError({
      message: `No T3 database found at ${sourceStateDir}/state.sqlite.`,
      suggestion: "Point --source at the legacy T3 home if it lives elsewhere.",
    });
  }

  const targetHasData = yield* exists(fs, path.join(targetStateDir, "state.sqlite"));
  if (targetHasData && !input.force) {
    return yield* new MigrateDataError({
      message: `Target ${targetStateDir} already contains data.`,
      suggestion: "Pass --force to overwrite it, or move the existing target aside.",
    });
  }

  // Copy the database plus its WAL/SHM siblings.
  for (const entry of joinStateEntries(sourceBaseDir, path.join, SQLITE_FILENAMES)) {
    if (yield* exists(fs, entry.from)) {
      yield* fs.makeDirectory(path.dirname(entry.to), { recursive: true }).pipe(Effect.ignore);
      yield* copyFile(fs, entry.from, entry.to);
    }
  }

  // Copy secrets and settings for a complete environment snapshot. Never
  // clobber a matching target file so a previous settings edit is preserved.
  for (const entry of joinStateEntries(sourceBaseDir, path.join, COPY_ENTRIES)) {
    if (yield* exists(fs, entry.from)) {
      if (yield* exists(fs, entry.to)) continue;
      yield* fs.makeDirectory(path.dirname(entry.to), { recursive: true }).pipe(Effect.ignore);
      yield* copyFile(fs, entry.from, entry.to);
    }
  }

  yield* Console.log(`Migrated T3 data from ${sourceStateDir} to ${targetStateDir}.`);
  yield* Console.log(
    "The next `t3 serve` / `t3 start` applies S5's schema migrations to the copied database automatically.",
  );
});

export const migrateDataCommand = Command.make("migrate-data", {
  source: Flag.string("source").pipe(
    Flag.withDescription("Legacy T3 home to copy from (default ~/.t3)."),
    Flag.optional,
  ),
  target: Flag.string("target").pipe(
    Flag.withDescription("S5 Code home to copy into (default ~/.s5code)."),
    Flag.optional,
  ),
  force: Flag.boolean("force").pipe(
    Flag.withDescription("Overwrite an existing non-empty target."),
    Flag.withDefault(false),
  ),
}).pipe(
  Command.withDescription(
    "One-time copy of existing T3 Code data into the S5 Code home so S5's migrations can run.",
  ),
  Command.withHandler((flags) =>
    Effect.gen(function* () {
      yield* migrateData({
        sourceHome: Option.getOrUndefined(flags.source),
        targetHome: Option.getOrUndefined(flags.target),
        force: flags.force,
      });
      return Option.none();
    }),
  ),
);
