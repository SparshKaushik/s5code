import * as NodeServices from "@effect/platform-node/NodeServices";
import { assert, describe, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Option from "effect/Option";
import * as Path from "effect/Path";

import { adoptLegacyT3HomeIfNeeded } from "./DesktopHomeAdoption.ts";

const SQLITE_HEADER = "SQLite format 3\0";

const withHome = <A, E>(
  run: (homeDirectory: string) => Effect.Effect<A, E, FileSystem.FileSystem | Path.Path>,
) =>
  Effect.gen(function* () {
    const fileSystem = yield* FileSystem.FileSystem;
    const homeDirectory = yield* fileSystem.makeTempDirectoryScoped({
      prefix: "s5-home-adoption-",
    });
    return yield* run(homeDirectory);
  }).pipe(Effect.provide(NodeServices.layer), Effect.scoped);

const seedLegacyHome = Effect.fn("seedLegacyHome")(function* (
  homeDirectory: string,
  contents: Record<string, string> = { "state.sqlite": SQLITE_HEADER },
) {
  const fileSystem = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const userdata = path.join(homeDirectory, ".t3", "userdata");
  yield* fileSystem.makeDirectory(userdata, { recursive: true });
  for (const [name, body] of Object.entries(contents)) {
    yield* fileSystem.writeFileString(path.join(userdata, name), body);
  }
});

describe("DesktopHomeAdoption", () => {
  it.effect("copies legacy t3 userdata into an empty s5code home without Clerk tokens", () =>
    withHome((homeDirectory) =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        yield* seedLegacyHome(homeDirectory, {
          "state.sqlite": SQLITE_HEADER,
          "settings.json": '{"ok":true}',
          "clerk-tokens.json": '{"__clerk_client_jwt":"enc:legacy-t3-session"}',
          "clerk-instance.json": '{"publishableKey":"pk_live_legacy"}',
        });

        yield* adoptLegacyT3HomeIfNeeded({
          homeDirectory,
          isDevelopment: false,
          isPackaged: true,
          t3Home: Option.none(),
        });

        const dest = path.join(homeDirectory, ".s5code", "userdata");
        assert.equal(
          yield* fileSystem.readFileString(path.join(dest, "state.sqlite")),
          SQLITE_HEADER,
        );
        assert.equal(
          yield* fileSystem.readFileString(path.join(dest, "settings.json")),
          '{"ok":true}',
        );
        assert.isTrue(
          yield* fileSystem.exists(path.join(homeDirectory, ".s5code", ".adopted-from-t3")),
        );
        assert.isTrue(
          yield* fileSystem.exists(path.join(homeDirectory, ".t3", "userdata", "state.sqlite")),
        );
        assert.isTrue(
          yield* fileSystem.exists(
            path.join(homeDirectory, ".t3", "userdata", "clerk-tokens.json"),
          ),
        );
        assert.isFalse(yield* fileSystem.exists(path.join(dest, "clerk-tokens.json")));
        assert.isFalse(yield* fileSystem.exists(path.join(dest, "clerk-instance.json")));
      }),
    ),
  );

  it.effect("does not overwrite an existing s5code state.sqlite", () =>
    withHome((homeDirectory) =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        yield* seedLegacyHome(homeDirectory, { "state.sqlite": SQLITE_HEADER });
        const dest = path.join(homeDirectory, ".s5code", "userdata");
        yield* fileSystem.makeDirectory(dest, { recursive: true });
        yield* fileSystem.writeFileString(path.join(dest, "state.sqlite"), "existing-db");

        yield* adoptLegacyT3HomeIfNeeded({
          homeDirectory,
          isDevelopment: false,
          isPackaged: true,
          t3Home: Option.none(),
        });

        assert.equal(
          yield* fileSystem.readFileString(path.join(dest, "state.sqlite")),
          "existing-db",
        );
        assert.isFalse(
          yield* fileSystem.exists(path.join(homeDirectory, ".s5code", ".adopted-from-t3")),
        );
      }),
    ),
  );

  it.effect("skips adoption when T3CODE_HOME is set", () =>
    withHome((homeDirectory) =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        yield* seedLegacyHome(homeDirectory);

        yield* adoptLegacyT3HomeIfNeeded({
          homeDirectory,
          isDevelopment: false,
          isPackaged: true,
          t3Home: Option.some(path.join(homeDirectory, "explicit-home")),
        });

        assert.isFalse(
          yield* fileSystem.exists(path.join(homeDirectory, ".s5code", "userdata", "state.sqlite")),
        );
      }),
    ),
  );

  it.effect("skips adoption in development and unpackaged builds", () =>
    withHome((homeDirectory) =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        yield* seedLegacyHome(homeDirectory);

        yield* adoptLegacyT3HomeIfNeeded({
          homeDirectory,
          isDevelopment: true,
          isPackaged: true,
          t3Home: Option.none(),
        });
        yield* adoptLegacyT3HomeIfNeeded({
          homeDirectory,
          isDevelopment: false,
          isPackaged: false,
          t3Home: Option.none(),
        });

        assert.isFalse(
          yield* fileSystem.exists(path.join(homeDirectory, ".s5code", "userdata", "state.sqlite")),
        );
      }),
    ),
  );

  it.effect("skips adoption when the marker already exists", () =>
    withHome((homeDirectory) =>
      Effect.gen(function* () {
        const fileSystem = yield* FileSystem.FileSystem;
        const path = yield* Path.Path;
        yield* seedLegacyHome(homeDirectory);
        yield* fileSystem.makeDirectory(path.join(homeDirectory, ".s5code"), { recursive: true });
        yield* fileSystem.writeFileString(
          path.join(homeDirectory, ".s5code", ".adopted-from-t3"),
          "1",
        );

        yield* adoptLegacyT3HomeIfNeeded({
          homeDirectory,
          isDevelopment: false,
          isPackaged: true,
          t3Home: Option.none(),
        });

        assert.isFalse(
          yield* fileSystem.exists(path.join(homeDirectory, ".s5code", "userdata", "state.sqlite")),
        );
      }),
    ),
  );
});
