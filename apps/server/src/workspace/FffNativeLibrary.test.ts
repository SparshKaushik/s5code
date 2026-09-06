import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";

import { HostProcessEnvironment } from "@t3tools/shared/hostProcess";

import {
  ensureFffNativeLibrary,
  extractFffNativeLibrary,
  FFF_NATIVE_LIB_PATH_ENV,
  type FffLibraryAsset,
} from "./FffNativeLibrary.ts";

const TestLayer = NodeServices.layer;

const asset = (name: string, contents: string): FffLibraryAsset => ({
  name,
  size: new TextEncoder().encode(contents).byteLength,
  arrayBuffer: () => Promise.resolve(new TextEncoder().encode(contents).buffer as ArrayBuffer),
});

const tempBaseDir = Effect.fn("tempBaseDir")(function* () {
  const fileSystem = yield* FileSystem.FileSystem;
  return yield* fileSystem.makeTempDirectoryScoped({ prefix: "t3code-fff-native-" });
});

it.layer(TestLayer, { excludeTestServices: true })("FffNativeLibrary", (it) => {
  it.effect("writes the library where a real dlopen can reach it", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const baseDir = yield* tempBaseDir();

      const target = yield* extractFffNativeLibrary({
        baseDir,
        asset: asset("libfff_c-abc123.so", "native-bytes"),
      });

      // Not inside bun's virtual filesystem, which is the whole point: the
      // dynamic loader is the kernel's and cannot read /$bunfs.
      expect(target).toBe(path.join(baseDir, "caches", "native", "libfff_c-abc123.so"));
      expect(yield* fileSystem.readFileString(target)).toBe("native-bytes");
      // dlopen needs the execute bit.
      const stat = yield* fileSystem.stat(target);
      expect(stat.mode & 0o111).not.toBe(0);
    }),
  );

  it.effect("leaves no partial file behind for a loader to find", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const baseDir = yield* tempBaseDir();

      yield* extractFffNativeLibrary({ baseDir, asset: asset("libfff_c-abc123.so", "bytes") });

      const entries = yield* fileSystem.readDirectory(path.join(baseDir, "caches", "native"));
      expect(entries).toEqual(["libfff_c-abc123.so"]);
    }),
  );

  it.effect("reuses an intact extraction rather than rewriting 5MB on every start", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const baseDir = yield* tempBaseDir();
      const library = asset("libfff_c-abc123.so", "native-bytes");

      const first = yield* extractFffNativeLibrary({ baseDir, asset: library });
      const firstStat = yield* fileSystem.stat(first);
      const second = yield* extractFffNativeLibrary({ baseDir, asset: library });

      expect(second).toBe(first);
      expect((yield* fileSystem.stat(second)).mtime).toEqual(firstStat.mtime);
    }),
  );

  it.effect("replaces a truncated extraction", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const baseDir = yield* tempBaseDir();
      const library = asset("libfff_c-abc123.so", "native-bytes");
      const target = path.join(baseDir, "caches", "native", library.name);

      // A previous start that died mid-write. Loading this would fail with a
      // message about file format that points nowhere near the cause.
      yield* fileSystem.makeDirectory(path.dirname(target), { recursive: true });
      yield* fileSystem.writeFileString(target, "trunc");

      yield* extractFffNativeLibrary({ baseDir, asset: library });
      expect(yield* fileSystem.readFileString(target)).toBe("native-bytes");
    }),
  );

  it.effect("stays out of the way when there is no embedded asset", () =>
    Effect.gen(function* () {
      const baseDir = yield* tempBaseDir();
      const env: NodeJS.ProcessEnv = {};

      yield* ensureFffNativeLibrary({ baseDir }).pipe(
        Effect.provideService(HostProcessEnvironment, env),
      );

      // `npx t3`, the desktop app, and a dev checkout all land here. Publishing a
      // path would override fff-node's own resolution, which is correct there.
      expect(env[FFF_NATIVE_LIB_PATH_ENV]).toBeUndefined();
    }),
  );

  it.effect("publishes the extracted path for fff-node to read", () =>
    Effect.gen(function* () {
      const path = yield* Path.Path;
      const baseDir = yield* tempBaseDir();
      const env: NodeJS.ProcessEnv = {};

      yield* ensureFffNativeLibrary({
        baseDir,
        asset: asset("libfff_c-abc123.so", "native-bytes"),
      }).pipe(Effect.provideService(HostProcessEnvironment, env));

      expect(env[FFF_NATIVE_LIB_PATH_ENV]).toBe(
        path.join(baseDir, "caches", "native", "libfff_c-abc123.so"),
      );
    }),
  );

  it.effect("keeps an operator's explicit path", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const baseDir = yield* tempBaseDir();
      const handBuilt = path.join(baseDir, "libfff_c.so");
      yield* fileSystem.writeFileString(handBuilt, "hand-built");
      const env: NodeJS.ProcessEnv = { [FFF_NATIVE_LIB_PATH_ENV]: handBuilt };

      yield* ensureFffNativeLibrary({ baseDir }).pipe(
        Effect.provideService(HostProcessEnvironment, env),
      );

      expect(env[FFF_NATIVE_LIB_PATH_ENV]).toBe(handBuilt);
    }),
  );

  it.effect("boots rather than failing when the cache is unwritable", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const baseDir = yield* tempBaseDir();
      // A file where the cache directory needs to be: mkdir cannot win.
      yield* fileSystem.writeFileString(path.join(baseDir, "caches"), "not a directory");
      const env: NodeJS.ProcessEnv = {};

      // No failure: an empty file tree is bad, a server that will not start is
      // worse.
      yield* ensureFffNativeLibrary({
        baseDir,
        asset: asset("libfff_c-abc123.so", "native-bytes"),
      }).pipe(Effect.provideService(HostProcessEnvironment, env));
      expect(env[FFF_NATIVE_LIB_PATH_ENV]).toBeUndefined();
    }),
  );
});
