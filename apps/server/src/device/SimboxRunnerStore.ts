import { ThreadId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as SimboxDeviceHost from "./SimboxDeviceHost.ts";

/**
 * Attached Simbox runners, so a server restart or turning device support back
 * on re-attaches them instead of losing them until the CLI registers again.
 * Registrations carry runner credentials, so the file is owner-only.
 */
export const StoredRunner = Schema.Struct({
  threadId: ThreadId,
  registration: SimboxDeviceHost.Registration,
});
export type StoredRunner = typeof StoredRunner.Type;

export interface SimboxRunnerStore {
  readonly load: Effect.Effect<ReadonlyArray<StoredRunner>>;
  readonly save: (runners: ReadonlyArray<StoredRunner>) => Effect.Effect<void>;
}

const StoredRunners = Schema.fromJsonString(Schema.Array(StoredRunner));
const decode = Schema.decodeUnknownEffect(StoredRunners);
const encode = Schema.encodeEffect(StoredRunners);

/** No persistence; used where the service has no state directory (tests). */
export const memory = (initial: ReadonlyArray<StoredRunner> = []): SimboxRunnerStore => {
  let runners = initial;
  return {
    load: Effect.sync(() => runners),
    save: (next) =>
      Effect.sync(() => {
        runners = next;
      }),
  };
};

export const file = Effect.fn("SimboxRunnerStore.file")(function* (location: string) {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const store: SimboxRunnerStore = {
    // An unreadable or outdated file only means nothing is restored.
    load: fs.readFileString(location).pipe(
      Effect.flatMap(decode),
      Effect.orElseSucceed((): ReadonlyArray<StoredRunner> => []),
    ),
    save: (runners) =>
      Effect.gen(function* () {
        const directory = path.dirname(location);
        yield* fs.makeDirectory(directory, { recursive: true });
        const content = yield* encode(runners);
        // Written beside the target so the rename is atomic, then the private
        // temp directory is removed whether or not the write landed.
        const temporary = yield* Effect.acquireRelease(
          fs.makeTempDirectory({ directory, prefix: ".simbox-runners-" }),
          (created) => fs.remove(created, { recursive: true }).pipe(Effect.ignore),
        );
        const staged = path.join(temporary, "runners.json");
        yield* fs.writeFileString(staged, content, { mode: 0o600 });
        yield* fs.rename(staged, location);
      }).pipe(Effect.scoped, Effect.ignore),
  };
  return store;
});
