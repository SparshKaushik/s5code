import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as Path from "effect/Path";

const SQLITE_HEADER = "SQLite format 3";
const ADOPTION_MARKER = ".adopted-from-t3";
const SKIP_ADOPTION_ENTRIES = new Set(["clerk-tokens.json", "clerk-instance.json"]);

const isReadableSqlite = Effect.fn("isReadableSqlite")(function* (filePath: string) {
  const fileSystem = yield* FileSystem.FileSystem;
  if (!(yield* fileSystem.exists(filePath))) {
    return false;
  }
  const bytes = yield* fileSystem.readFile(filePath);
  if (bytes.byteLength < 16) {
    return false;
  }
  return new TextDecoder().decode(bytes.subarray(0, 15)) === SQLITE_HEADER;
});

export const adoptLegacyT3HomeIfNeeded = Effect.fn("adoptLegacyT3HomeIfNeeded")(function* (input: {
  readonly homeDirectory: string;
  readonly isDevelopment: boolean;
  readonly isPackaged: boolean;
  readonly t3Home: Option.Option<string>;
}) {
  if (input.isDevelopment || !input.isPackaged || Option.isSome(input.t3Home)) {
    return;
  }

  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const currentBase = path.join(input.homeDirectory, ".s5code");
  const markerPath = path.join(currentBase, ADOPTION_MARKER);
  if (yield* fileSystem.exists(markerPath)) {
    return;
  }

  const destUserdata = path.join(currentBase, "userdata");
  const destSqlite = path.join(destUserdata, "state.sqlite");
  if (yield* fileSystem.exists(destSqlite)) {
    return;
  }

  const sourceUserdata = path.join(input.homeDirectory, ".t3", "userdata");
  const sourceSqlite = path.join(sourceUserdata, "state.sqlite");
  if (!(yield* isReadableSqlite(sourceSqlite))) {
    return;
  }

  yield* fileSystem.makeDirectory(destUserdata, { recursive: true });
  const entries = yield* fileSystem.readDirectory(sourceUserdata);
  for (const entry of entries) {
    if (SKIP_ADOPTION_ENTRIES.has(entry)) {
      continue;
    }
    const fromPath = path.join(sourceUserdata, entry);
    const toPath = path.join(destUserdata, entry);
    if (!(yield* fileSystem.exists(toPath))) {
      yield* fileSystem.copy(fromPath, toPath);
    }
  }

  yield* fileSystem.writeFileString(markerPath, "1");
  yield* Effect.log("Adopted legacy T3 home into S5 Code home").pipe(
    Effect.annotateLogs({ from: sourceUserdata, to: destUserdata }),
  );
});
