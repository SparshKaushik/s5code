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
        yield* fs.makeDirectory(path.dirname(location), { recursive: true });
        const content = yield* encode(runners);
        const temporary = yield* fs.makeTempFile({
          directory: path.dirname(location),
          prefix: ".simbox-runners-",
        });
        yield* Effect.gen(function* () {
          yield* fs.chmod(temporary, 0o600);
          yield* fs.writeFileString(temporary, content);
          yield* fs.rename(temporary, location);
        }).pipe(Effect.ensuring(fs.remove(temporary, { force: true }).pipe(Effect.ignore)));
      }).pipe(Effect.ignore),
  };
  return store;
});
