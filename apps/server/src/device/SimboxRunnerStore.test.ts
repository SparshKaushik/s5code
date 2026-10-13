import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import { ThreadId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as SimboxRunnerStore from "./SimboxRunnerStore.ts";

const runner: SimboxRunnerStore.StoredRunner = {
  threadId: ThreadId.make("thread-1"),
  registration: {
    version: 1,
    createdAt: 0,
    runId: "039dd761-9144-4a9c-a03b-e44c893d73ad",
    tunnelUrl: "https://r-0123456789abcdef.simboxtouchtech.dpdns.org",
    daemonToken: "private-runner-token",
    expiresAt: null,
    platform: "ios",
    stop: false,
    autoBoot: true,
    apiUrl: "https://api.simbox.touchtech.club",
    userToken: "private-app-token",
  },
};

it.layer(NodeServices.layer)("Simbox runner store", (it) => {
  it.effect("persists runners privately and treats an unreadable file as empty", () =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const directory = yield* fs.makeTempDirectoryScoped();
      const file = path.join(directory, "device", "simbox-runners.json");
      const store = yield* SimboxRunnerStore.file(file);
      expect(yield* store.load).toEqual([]);
      yield* store.save([runner]);
      expect(yield* (yield* SimboxRunnerStore.file(file)).load).toEqual([runner]);
      expect((yield* fs.stat(file)).mode & 0o777).toBe(0o600);
      yield* fs.writeFileString(file, "{not json");
      expect(yield* store.load).toEqual([]);
    }).pipe(Effect.scoped),
  );
});
