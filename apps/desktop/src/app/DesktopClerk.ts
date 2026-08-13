import { createClerkBridge } from "@clerk/electron";
import { storage } from "@clerk/electron/storage";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Scope from "effect/Scope";
import * as Electron from "electron";

import { clerkFrontendApiHostnameFromPublishableKey } from "@t3tools/shared/relayAuth";
import * as ElectronApp from "../electron/ElectronApp.ts";
import * as ElectronProtocol from "../electron/ElectronProtocol.ts";
import * as ElectronWindow from "../electron/ElectronWindow.ts";
import * as DesktopAppIdentity from "./DesktopAppIdentity.ts";
import * as DesktopEnvironment from "./DesktopEnvironment.ts";

declare const __T3CODE_BUILD_CLERK_PUBLISHABLE_KEY__: string | undefined;

export class DesktopClerkBridgeInitializationError extends Schema.TaggedErrorClass<DesktopClerkBridgeInitializationError>()(
  "DesktopClerkBridgeInitializationError",
  {
    stateDir: Schema.String,
    isDevelopment: Schema.Boolean,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to initialize the desktop Clerk bridge for state directory "${this.stateDir}" (development: ${this.isDevelopment}).`;
  }
}

export class DesktopClerkBridgeCleanupError extends Schema.TaggedErrorClass<DesktopClerkBridgeCleanupError>()(
  "DesktopClerkBridgeCleanupError",
  {
    stateDir: Schema.String,
    isDevelopment: Schema.Boolean,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to clean up the desktop Clerk bridge for state directory "${this.stateDir}" (development: ${this.isDevelopment}).`;
  }
}

export class DesktopClerk extends Context.Service<
  DesktopClerk,
  {
    readonly configure: Effect.Effect<
      void,
      never,
      ElectronApp.ElectronApp | ElectronWindow.ElectronWindow | Scope.Scope
    >;
  }
>()("@t3tools/desktop/app/DesktopClerk") {}

export function resolveDesktopClerkFrontendApiHostname(
  publishableKey: string | undefined,
): string | undefined {
  const normalizedKey = publishableKey?.trim();
  if (!normalizedKey) return undefined;

  try {
    return clerkFrontendApiHostnameFromPublishableKey(normalizedKey);
  } catch {
    return undefined;
  }
}

export const desktopClerkFrontendApiHostname = resolveDesktopClerkFrontendApiHostname(
  typeof __T3CODE_BUILD_CLERK_PUBLISHABLE_KEY__ === "undefined"
    ? undefined
    : __T3CODE_BUILD_CLERK_PUBLISHABLE_KEY__,
);

const CLERK_TOKENS_FILE = "clerk-tokens.json";
const CLERK_INSTANCE_FILE = "clerk-instance.json";

const readStoredPublishableKey = Effect.fn("desktop.clerk.readStoredPublishableKey")(function* (
  instancePath: string,
) {
  const fileSystem = yield* FileSystem.FileSystem;
  if (!(yield* fileSystem.exists(instancePath))) {
    return null;
  }
  const raw = yield* fileSystem.readFileString(instancePath);
  try {
    const parsed: unknown = JSON.parse(raw);
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      "publishableKey" in parsed &&
      typeof parsed.publishableKey === "string"
    ) {
      return parsed.publishableKey.trim() || null;
    }
  } catch {
    return null;
  }
  return null;
});

export const discardIncompatibleClerkTokens = Effect.fn("desktop.clerk.discardIncompatibleTokens")(
  function* (input: { readonly stateDir: string; readonly publishableKey: string | undefined }) {
    const publishableKey = input.publishableKey?.trim() ?? "";
    if (!publishableKey) {
      return;
    }

    const fileSystem = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const tokensPath = path.join(input.stateDir, CLERK_TOKENS_FILE);
    const instancePath = path.join(input.stateDir, CLERK_INSTANCE_FILE);
    const storedKey = yield* readStoredPublishableKey(instancePath);
    if (storedKey === publishableKey) {
      return;
    }

    if (yield* fileSystem.exists(tokensPath)) {
      yield* fileSystem.remove(tokensPath, { force: true });
      yield* Effect.log("Discarded Clerk tokens from a different Clerk instance").pipe(
        Effect.annotateLogs({ stateDir: input.stateDir }),
      );
    }

    yield* fileSystem.writeFileString(instancePath, JSON.stringify({ publishableKey }));
  },
);

export function clerkSatelliteHttpsOriginFromFrontendApiHostname(
  hostname: string | undefined,
): string | undefined {
  const satellite = hostname?.replace(/^clerk\./, "");
  if (!satellite || satellite === hostname) {
    return undefined;
  }
  return `https://${satellite}`;
}

export function rewriteCustomSchemeOriginHeader(
  requestHeaders: Record<string, string>,
  allowedHttpsOrigin: string,
): Record<string, string> {
  const originKey = Object.keys(requestHeaders).find((key) => key.toLowerCase() === "origin");
  if (originKey === undefined) {
    return requestHeaders;
  }
  const origin = requestHeaders[originKey];
  if (origin === undefined || origin.startsWith("http://") || origin.startsWith("https://")) {
    return requestHeaders;
  }
  const authorizationKey = Object.keys(requestHeaders).find(
    (key) => key.toLowerCase() === "authorization",
  );
  if (authorizationKey !== undefined && requestHeaders[authorizationKey]) {
    const next = { ...requestHeaders };
    delete next[originKey];
    return next;
  }
  return { ...requestHeaders, [originKey]: allowedHttpsOrigin };
}

export function rewriteClerkCorsOrigin(
  responseHeaders: Record<string, string[]>,
  rendererOrigin: string,
): Record<string, string[]> {
  const next = { ...responseHeaders };
  const existingKey = Object.keys(next).find(
    (key) => key.toLowerCase() === "access-control-allow-origin",
  );
  if (existingKey !== undefined) {
    delete next[existingKey];
  }
  next["Access-Control-Allow-Origin"] = [rendererOrigin];
  return next;
}

export function installClerkDesktopOriginFilter(
  session: {
    readonly webRequest: {
      readonly onBeforeSendHeaders: (
        filter: { readonly urls: readonly string[] },
        listener: (
          details: { readonly id: number; readonly requestHeaders: Record<string, string> },
          callback: (response: { readonly requestHeaders: Record<string, string> }) => void,
        ) => void,
      ) => void;
      readonly onHeadersReceived: (
        filter: { readonly urls: readonly string[] },
        listener: (
          details: { readonly id: number; readonly responseHeaders?: Record<string, string[]> },
          callback: (response: { readonly responseHeaders?: Record<string, string[]> }) => void,
        ) => void,
      ) => void;
    };
  },
  clerkFrontendApiHostname: string | undefined,
) {
  const allowedHttpsOrigin =
    clerkSatelliteHttpsOriginFromFrontendApiHostname(clerkFrontendApiHostname);
  if (!clerkFrontendApiHostname || !allowedHttpsOrigin) {
    return;
  }

  const urls = { urls: [`https://${clerkFrontendApiHostname}/*`] };
  const rewrittenOrigins = new Map<number, string>();

  session.webRequest.onBeforeSendHeaders(urls, (details, callback) => {
    const originKey = Object.keys(details.requestHeaders).find(
      (key) => key.toLowerCase() === "origin",
    );
    const origin = originKey === undefined ? undefined : details.requestHeaders[originKey];
    if (origin !== undefined && !origin.startsWith("http://") && !origin.startsWith("https://")) {
      rewrittenOrigins.set(details.id, origin);
    }
    callback({
      requestHeaders: rewriteCustomSchemeOriginHeader(details.requestHeaders, allowedHttpsOrigin),
    });
  });

  session.webRequest.onHeadersReceived(urls, (details, callback) => {
    const rendererOrigin = rewrittenOrigins.get(details.id);
    rewrittenOrigins.delete(details.id);
    if (rendererOrigin === undefined || details.responseHeaders === undefined) {
      callback({ responseHeaders: details.responseHeaders });
      return;
    }
    callback({
      responseHeaders: rewriteClerkCorsOrigin(details.responseHeaders, rendererOrigin),
    });
  });
}

export function createDesktopClerkBridge(stateDir: string, isDevelopment: boolean) {
  return createClerkBridge({
    storage: storage({ path: stateDir }),
    passkeys: true,
    renderer: {
      scheme: ElectronProtocol.getDesktopScheme(isDevelopment),
      host: ElectronProtocol.DESKTOP_HOST,
    },
  });
}

export const make = Effect.gen(function* () {
  const environment = yield* DesktopEnvironment.DesktopEnvironment;
  const electronApp = yield* ElectronApp.ElectronApp;
  yield* discardIncompatibleClerkTokens({
    stateDir: environment.stateDir,
    publishableKey:
      typeof __T3CODE_BUILD_CLERK_PUBLISHABLE_KEY__ === "undefined"
        ? undefined
        : __T3CODE_BUILD_CLERK_PUBLISHABLE_KEY__,
  }).pipe(
    Effect.catch((error) =>
      Effect.logWarning("Could not reconcile Clerk token instance").pipe(
        Effect.annotateLogs({ error: String(error) }),
      ),
    ),
  );

  // Electron scopes the single-instance lock to the userData directory and
  // creates that directory when the lock is acquired. The SDK bridge takes
  // the lock at creation, so userData must already point at the real
  // directory here — under the default productName-derived path, acquiring
  // the lock would create "S5 Code (Alpha)" and make the legacy-install
  // detection in resolveUserDataPath match on fresh installs.
  const userDataPath = yield* DesktopAppIdentity.resolveUserDataPath;
  yield* electronApp.setPath("userData", userDataPath);
  yield* Effect.sync(() => {
    try {
      installClerkDesktopOriginFilter(
        Electron.session.defaultSession,
        desktopClerkFrontendApiHostname,
      );
    } catch {
      return;
    }
  });

  const bridge = yield* Effect.acquireRelease(
    Effect.try({
      try: () => createDesktopClerkBridge(environment.stateDir, environment.isDevelopment),
      catch: (cause) =>
        new DesktopClerkBridgeInitializationError({
          stateDir: environment.stateDir,
          isDevelopment: environment.isDevelopment,
          cause,
        }),
    }),
    (bridge) =>
      Effect.try({
        try: () => bridge.cleanup(),
        catch: (cause) =>
          new DesktopClerkBridgeCleanupError({
            stateDir: environment.stateDir,
            isDevelopment: environment.isDevelopment,
            cause,
          }),
      }).pipe(Effect.orDie),
  );

  return DesktopClerk.of({
    configure: Effect.gen(function* () {
      const electronApp = yield* ElectronApp.ElectronApp;
      const electronWindow = yield* ElectronWindow.ElectronWindow;
      const context = yield* Effect.context<ElectronWindow.ElectronWindow>();
      const runPromise = Effect.runPromiseWith(context);

      // The SDK bridge holds Electron's single-instance lock (acquired at
      // bridge creation) so OAuth deep-link callbacks on Windows/Linux are
      // forwarded to the running app. In a secondary instance the bridge has
      // already begun quitting the app; app.quit() is asynchronous, so stop
      // bootstrap here before whenReady can fire.
      if (!bridge.isPrimaryInstance) {
        yield* electronApp.quit;
        return yield* Effect.interrupt;
      }

      yield* electronApp.on("second-instance", () => {
        void runPromise(
          Effect.gen(function* () {
            const mainWindow = yield* electronWindow.currentMainOrFirst;
            if (Option.isSome(mainWindow)) {
              yield* electronWindow.reveal(mainWindow.value);
            }
          }),
        );
      });
    }).pipe(Effect.withSpan("desktop.clerk.configure")),
  });
});

export const layer = Layer.effect(DesktopClerk, make);
