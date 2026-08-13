import * as NodeServices from "@effect/platform-node/NodeServices";
import { assert, describe, it } from "@effect/vitest";
import * as Cause from "effect/Cause";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { beforeEach, vi } from "vite-plus/test";

const { createClerkBridgeMock, storageAdapter, storageMock } = vi.hoisted(() => ({
  createClerkBridgeMock: vi.fn(),
  storageAdapter: {
    getItem: vi.fn(),
    setItem: vi.fn(),
    removeItem: vi.fn(),
  },
  storageMock: vi.fn(),
}));

vi.mock("@clerk/electron", () => ({
  createClerkBridge: createClerkBridgeMock,
}));

vi.mock("@clerk/electron/storage", () => ({
  storage: storageMock,
}));

import * as Exit from "effect/Exit";
import * as FileSystem from "effect/FileSystem";
import * as ElectronApp from "../electron/ElectronApp.ts";
import * as ElectronWindow from "../electron/ElectronWindow.ts";
import * as DesktopClerk from "./DesktopClerk.ts";
import * as DesktopEnvironment from "./DesktopEnvironment.ts";

const makeDesktopClerkLayer = (isDevelopment = true, events: string[] = []) => {
  const environment = DesktopEnvironment.DesktopEnvironment.of({
    stateDir: "/tmp/t3-state",
    isDevelopment,
    appDataDirectory: "/tmp/app-data",
    userDataDirName: isDevelopment ? "s5code-dev" : "s5code",
    legacyUserDataDirName: isDevelopment ? "t3code-dev" : "t3code",
    path: { join: (...parts: ReadonlyArray<string>) => parts.join("/") },
  } as unknown as DesktopEnvironment.DesktopEnvironment["Service"]);

  const electronApp = {
    setPath: (name: string, value: string) =>
      Effect.sync(() => {
        events.push(`setPath:${name}:${value}`);
      }),
  } as unknown as ElectronApp.ElectronApp["Service"];

  return DesktopClerk.layer.pipe(
    Layer.provide(
      Layer.mergeAll(
        Layer.succeed(DesktopEnvironment.DesktopEnvironment, environment),
        Layer.succeed(ElectronApp.ElectronApp, electronApp),
        FileSystem.layerNoop({ exists: () => Effect.succeed(false) }),
      ),
    ),
  );
};

describe("DesktopClerk", () => {
  beforeEach(() => {
    createClerkBridgeMock.mockReset();
    storageMock.mockReset();
  });

  it("rewrites custom-scheme Origin headers to the Clerk satellite HTTPS origin", () => {
    assert.deepEqual(
      DesktopClerk.rewriteCustomSchemeOriginHeader(
        {
          Origin: "s5code://app",
          Authorization: "Bearer native-token",
        },
        "https://s5code.touchtech.club",
      ),
      { Authorization: "Bearer native-token" },
    );
    assert.deepEqual(
      DesktopClerk.rewriteCustomSchemeOriginHeader(
        { Origin: "s5code://app" },
        "https://s5code.touchtech.club",
      ),
      { Origin: "https://s5code.touchtech.club" },
    );
    assert.deepEqual(
      DesktopClerk.rewriteCustomSchemeOriginHeader(
        { origin: "t3code://app" },
        "https://s5code.touchtech.club",
      ),
      { origin: "https://s5code.touchtech.club" },
    );
    assert.deepEqual(
      DesktopClerk.rewriteCustomSchemeOriginHeader(
        {
          Origin: "https://s5code.touchtech.club",
          Accept: "application/json",
        },
        "https://example.invalid",
      ),
      {
        Origin: "https://s5code.touchtech.club",
        Accept: "application/json",
      },
    );
  });

  it("derives the satellite HTTPS origin Clerk accepts from the Frontend API host", () => {
    assert.equal(
      DesktopClerk.clerkSatelliteHttpsOriginFromFrontendApiHostname("clerk.s5code.touchtech.club"),
      "https://s5code.touchtech.club",
    );
    assert.equal(
      DesktopClerk.clerkSatelliteHttpsOriginFromFrontendApiHostname("example.clerk.accounts.dev"),
      undefined,
    );
  });

  it("restores the renderer origin on Clerk CORS responses after rewriting Origin", () => {
    assert.deepEqual(
      DesktopClerk.rewriteClerkCorsOrigin(
        { "Access-Control-Allow-Origin": ["https://s5code.touchtech.club"] },
        "s5code://app",
      ),
      { "Access-Control-Allow-Origin": ["s5code://app"] },
    );
    assert.deepEqual(
      DesktopClerk.rewriteClerkCorsOrigin(
        { "access-control-allow-origin": ["https://s5code.touchtech.club"] },
        "s5code-dev://app",
      ),
      { "Access-Control-Allow-Origin": ["s5code-dev://app"] },
    );
  });

  it("derives the Clerk Frontend API hostname used by the desktop CSP", () => {
    const publishableKey = `pk_test_${btoa("clerk.t3.codes$")}`;

    assert.equal(
      DesktopClerk.resolveDesktopClerkFrontendApiHostname(publishableKey),
      "clerk.t3.codes",
    );
    assert.equal(DesktopClerk.resolveDesktopClerkFrontendApiHostname(""), undefined);
    assert.equal(DesktopClerk.resolveDesktopClerkFrontendApiHostname("invalid"), undefined);
  });

  it.effect("acquires and releases the SDK bridge with the layer", () => {
    const cleanup = vi.fn();
    const events: string[] = [];
    storageMock.mockReturnValue(storageAdapter);
    createClerkBridgeMock.mockImplementation(() => {
      events.push("createClerkBridge");
      return { cleanup, isPrimaryInstance: true };
    });

    return Effect.gen(function* () {
      yield* Effect.scoped(Layer.build(makeDesktopClerkLayer(true, events)));

      assert.deepEqual(createClerkBridgeMock.mock.calls, [
        [
          {
            storage: storageAdapter,
            passkeys: true,
            renderer: { scheme: "s5code-dev", host: "app" },
          },
        ],
      ]);
      assert.equal(cleanup.mock.calls.length, 1);
      // The bridge acquires Electron's single-instance lock at creation, and
      // the lock both lives in and creates the userData directory — so the
      // real path must be set before the bridge exists.
      assert.deepEqual(events, ["setPath:userData:/tmp/app-data/s5code-dev", "createClerkBridge"]);
      storageMock.mockClear();
      createClerkBridgeMock.mockClear();
    });
  });

  it.effect("preserves bridge initialization failures", () => {
    const cause = new Error("bridge initialization failed");
    storageMock.mockReturnValue(storageAdapter);
    createClerkBridgeMock.mockImplementationOnce(() => {
      throw cause;
    });

    return Effect.gen(function* () {
      const error = yield* Effect.scoped(Layer.build(makeDesktopClerkLayer())).pipe(Effect.flip);

      assert.instanceOf(error, DesktopClerk.DesktopClerkBridgeInitializationError);
      assert.equal(error.stateDir, "/tmp/t3-state");
      assert.equal(error.isDevelopment, true);
      assert.strictEqual(error.cause, cause);
      assert.equal(
        error.message,
        'Failed to initialize the desktop Clerk bridge for state directory "/tmp/t3-state" (development: true).',
      );
    });
  });

  it.effect("preserves bridge cleanup failures", () => {
    const cause = new Error("bridge cleanup failed");
    storageMock.mockReturnValue(storageAdapter);
    createClerkBridgeMock.mockReturnValue({
      cleanup: () => {
        throw cause;
      },
    });

    return Effect.gen(function* () {
      const exit = yield* Effect.exit(Effect.scoped(Layer.build(makeDesktopClerkLayer(false))));

      assert.equal(exit._tag, "Failure");
      if (exit._tag === "Failure") {
        const error = Cause.squash(exit.cause);
        assert.instanceOf(error, DesktopClerk.DesktopClerkBridgeCleanupError);
        assert.equal(error.stateDir, "/tmp/t3-state");
        assert.equal(error.isDevelopment, false);
        assert.strictEqual(error.cause, cause);
        assert.equal(
          error.message,
          'Failed to clean up the desktop Clerk bridge for state directory "/tmp/t3-state" (development: false).',
        );
      }
    });
  });

  it.effect("registers the second-instance handler in the primary instance", () => {
    storageMock.mockReturnValue(storageAdapter);
    createClerkBridgeMock.mockReturnValue({ cleanup: vi.fn(), isPrimaryInstance: true });
    const quit = vi.fn();
    const registeredEvents: string[] = [];
    const electronApp = {
      quit: Effect.sync(quit),
      on: (eventName: string) =>
        Effect.sync(() => {
          registeredEvents.push(eventName);
        }),
    } as unknown as ElectronApp.ElectronApp["Service"];
    const electronWindow = {} as ElectronWindow.ElectronWindow["Service"];

    return Effect.gen(function* () {
      const clerk = yield* DesktopClerk.DesktopClerk;
      const exit = yield* Effect.exit(Effect.scoped(clerk.configure));

      assert.isTrue(Exit.isSuccess(exit));
      assert.equal(quit.mock.calls.length, 0);
      assert.deepEqual(registeredEvents, ["second-instance"]);
    }).pipe(
      Effect.provide(makeDesktopClerkLayer()),
      Effect.provideService(ElectronApp.ElectronApp, electronApp),
      Effect.provideService(ElectronWindow.ElectronWindow, electronWindow),
    );
  });

  it.effect("quits and interrupts startup in a secondary instance", () => {
    storageMock.mockReturnValue(storageAdapter);
    createClerkBridgeMock.mockReturnValue({ cleanup: vi.fn(), isPrimaryInstance: false });
    const quit = vi.fn();
    const registeredEvents: string[] = [];
    const electronApp = {
      quit: Effect.sync(quit),
      on: (eventName: string) =>
        Effect.sync(() => {
          registeredEvents.push(eventName);
        }),
    } as unknown as ElectronApp.ElectronApp["Service"];
    const electronWindow = {} as ElectronWindow.ElectronWindow["Service"];

    return Effect.gen(function* () {
      const clerk = yield* DesktopClerk.DesktopClerk;
      const exit = yield* Effect.exit(Effect.scoped(clerk.configure));

      assert.isTrue(Exit.hasInterrupts(exit));
      assert.equal(quit.mock.calls.length, 1);
      assert.deepEqual(registeredEvents, []);
    }).pipe(
      Effect.provide(makeDesktopClerkLayer()),
      Effect.provideService(ElectronApp.ElectronApp, electronApp),
      Effect.provideService(ElectronWindow.ElectronWindow, electronWindow),
    );
  });

  it.each([
    { isDevelopment: true, scheme: "s5code-dev" },
    { isDevelopment: false, scheme: "s5code" },
  ])("configures the SDK with the $scheme renderer origin", ({ isDevelopment, scheme }) => {
    const bridge = { cleanup: vi.fn(), isPrimaryInstance: true };
    storageMock.mockReturnValue(storageAdapter);
    createClerkBridgeMock.mockReturnValue(bridge);

    assert.equal(DesktopClerk.createDesktopClerkBridge("/tmp/t3-state", isDevelopment), bridge);
    assert.deepEqual(storageMock.mock.calls, [[{ path: "/tmp/t3-state" }]]);
    assert.deepEqual(createClerkBridgeMock.mock.calls, [
      [
        {
          storage: storageAdapter,
          passkeys: true,
          renderer: { scheme, host: "app" },
        },
      ],
    ]);
    storageMock.mockClear();
    createClerkBridgeMock.mockClear();
  });

  it.effect("discards Clerk tokens copied from a different Clerk instance", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const stateDir = yield* fileSystem.makeTempDirectoryScoped({
        prefix: "s5-clerk-tokens-",
      });
      const tokensPath = `${stateDir}/clerk-tokens.json`;
      const instancePath = `${stateDir}/clerk-instance.json`;
      yield* fileSystem.writeFileString(
        tokensPath,
        '{"__clerk_client_jwt":"enc:legacy-t3-session"}',
      );

      yield* DesktopClerk.discardIncompatibleClerkTokens({
        stateDir,
        publishableKey: "pk_live_s5code",
      });

      assert.isFalse(yield* fileSystem.exists(tokensPath));
      assert.equal(
        yield* fileSystem.readFileString(instancePath),
        JSON.stringify({ publishableKey: "pk_live_s5code" }),
      );
    }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );

  it.effect("keeps Clerk tokens that already belong to this instance", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const stateDir = yield* fileSystem.makeTempDirectoryScoped({
        prefix: "s5-clerk-tokens-",
      });
      const tokensPath = `${stateDir}/clerk-tokens.json`;
      yield* fileSystem.writeFileString(tokensPath, '{"__clerk_client_jwt":"enc:current"}');
      yield* fileSystem.writeFileString(
        `${stateDir}/clerk-instance.json`,
        JSON.stringify({ publishableKey: "pk_live_s5code" }),
      );

      yield* DesktopClerk.discardIncompatibleClerkTokens({
        stateDir,
        publishableKey: "pk_live_s5code",
      });

      assert.equal(
        yield* fileSystem.readFileString(tokensPath),
        '{"__clerk_client_jwt":"enc:current"}',
      );
    }).pipe(Effect.provide(NodeServices.layer), Effect.scoped),
  );
});
