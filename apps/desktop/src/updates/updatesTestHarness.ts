import * as NodeServices from "@effect/platform-node/NodeServices";
import type { DesktopUpdateState } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";

import * as DesktopBackendPool from "../backend/DesktopBackendPool.ts";
import * as DesktopConfig from "../app/DesktopConfig.ts";
import * as DesktopEnvironment from "../app/DesktopEnvironment.ts";
import * as ElectronApp from "../electron/ElectronApp.ts";
import * as ElectronUpdater from "../electron/ElectronUpdater.ts";
import * as ElectronWindow from "../electron/ElectronWindow.ts";
import * as DesktopAppSettings from "../settings/DesktopAppSettings.ts";
import * as DesktopState from "../app/DesktopState.ts";
import * as DesktopWindow from "../window/DesktopWindow.ts";
import * as DesktopUpdates from "./DesktopUpdates.ts";
import * as MacUnsignedUpdateInstall from "./MacUnsignedUpdateInstall.ts";

/** Shared DesktopUpdates test harness: a fully stubbed updater layer whose
    electron-updater events are driven by hand via `emit`. Used by
    DesktopUpdates.test.ts and DesktopRemoteUpdates.test.ts. */

export const flushCallbacks = Effect.yieldNow;

export interface UpdatesHarnessOptions {
  readonly checkForUpdates?: Effect.Effect<
    void,
    ElectronUpdater.ElectronUpdaterCheckForUpdatesError
  >;
  readonly beforeSetUpdateChannel?: Effect.Effect<void>;
  readonly setUpdateChannelError?: DesktopAppSettings.DesktopSettingsWriteError;
  readonly setDisableDifferentialDownload?: Effect.Effect<void>;
  readonly downloadUpdate?: Effect.Effect<void | ReadonlyArray<string>>;
  readonly quitAndInstall?: Effect.Effect<void, ElectronUpdater.ElectronUpdaterQuitAndInstallError>;
  readonly stopBackend?: Effect.Effect<void>;
  readonly startBackend?: Effect.Effect<void>;
  readonly squirrelCompatibleSignature?: boolean;
  readonly unsignedMacInstall?: Effect.Effect<
    void,
    MacUnsignedUpdateInstall.MacUnsignedUpdateInstallError
  >;
  readonly appPath?: string;
  readonly env?: Record<string, string | undefined>;
}

export function makeHarness(options: UpdatesHarnessOptions = {}) {
  let checkCount = 0;
  let quitAndInstallCount = 0;
  let downloadCount = 0;
  let allowDowngrade = false;
  let fullChangelog = false;
  let appQuitCount = 0;
  let backendStartCount = 0;
  let destroyAllCount = 0;
  let revealOrCreateMainCount = 0;
  const bundleReplaceCalls: Array<{
    readonly downloadedZipPath: string;
    readonly appPath: string;
  }> = [];
  const feedUrls: ElectronUpdater.ElectronUpdaterFeedUrl[] = [];
  const listeners = new Map<string, Set<(...args: readonly unknown[]) => void>>();
  const sentStates: DesktopUpdateState[] = [];
  const installSteps: string[] = [];

  const addListener = (eventName: string, listener: (...args: readonly unknown[]) => void) => {
    const eventListeners = listeners.get(eventName) ?? new Set();
    eventListeners.add(listener);
    listeners.set(eventName, eventListeners);
  };

  const removeListener = (eventName: string, listener: (...args: readonly unknown[]) => void) => {
    const eventListeners = listeners.get(eventName);
    if (!eventListeners) {
      return;
    }
    eventListeners.delete(listener);
    if (eventListeners.size === 0) {
      listeners.delete(eventName);
    }
  };

  const updaterLayer = Layer.succeed(ElectronUpdater.ElectronUpdater, {
    setFeedURL: (options) =>
      Effect.sync(() => {
        feedUrls.push(options);
      }),
    setAutoDownload: () => Effect.void,
    setAutoInstallOnAppQuit: () => Effect.void,
    setChannel: () => Effect.void,
    setAllowPrerelease: () => Effect.void,
    allowDowngrade: Effect.sync(() => allowDowngrade),
    setAllowDowngrade: (value) =>
      Effect.sync(() => {
        allowDowngrade = value;
      }),
    setFullChangelog: (value) =>
      Effect.sync(() => {
        fullChangelog = value;
      }),
    setDisableDifferentialDownload: () => options.setDisableDifferentialDownload ?? Effect.void,
    checkForUpdates: Effect.sync(() => {
      checkCount += 1;
    }).pipe(Effect.andThen(options.checkForUpdates ?? Effect.void)),
    downloadUpdate: Effect.sync(() => {
      downloadCount += 1;
    }).pipe(
      Effect.andThen(options.downloadUpdate ?? Effect.void),
      Effect.map((downloadedFiles) => downloadedFiles ?? []),
    ),
    quitAndInstall: () =>
      Effect.sync(() => {
        quitAndInstallCount += 1;
        installSteps.push("quitAndInstall");
      }).pipe(Effect.andThen(options.quitAndInstall ?? Effect.void)),
    on: (eventName, listener) =>
      Effect.acquireRelease(
        Effect.sync(() => {
          addListener(eventName, listener as unknown as (...args: readonly unknown[]) => void);
        }),
        () =>
          Effect.sync(() => {
            removeListener(eventName, listener as unknown as (...args: readonly unknown[]) => void);
          }),
      ).pipe(Effect.asVoid),
  } satisfies ElectronUpdater.ElectronUpdater["Service"]);

  const windowLayer = Layer.succeed(ElectronWindow.ElectronWindow, {
    create: () => Effect.die("unexpected BrowserWindow creation"),
    main: Effect.succeed(Option.none()),
    currentMainOrFirst: Effect.succeed(Option.none()),
    focusedMainOrFirst: Effect.succeed(Option.none()),
    setMain: () => Effect.void,
    clearMain: () => Effect.void,
    reveal: () => Effect.void,
    sendAll: (_channel, state) =>
      Effect.sync(() => {
        sentStates.push(state as DesktopUpdateState);
      }),
    destroyAll: Effect.sync(() => {
      installSteps.push("destroyAll");
      destroyAllCount += 1;
    }),
    syncAllAppearance: () => Effect.void,
  } satisfies ElectronWindow.ElectronWindow["Service"]);

  const stubBackendInstance: DesktopBackendPool.DesktopBackendInstance = {
    id: DesktopBackendPool.PRIMARY_INSTANCE_ID,
    label: Effect.succeed("Windows"),
    start: Effect.sync(() => {
      installSteps.push("startBackend");
      backendStartCount += 1;
    }).pipe(Effect.andThen(options.startBackend ?? Effect.void)),
    stop: () => options.stopBackend ?? Effect.void,
    currentConfig: Effect.succeed(Option.none()),
    snapshot: Effect.succeed({
      desiredRunning: false,
      ready: false,
      activePid: Option.none(),
      restartAttempt: 0,
      restartScheduled: false,
    }),
    waitForReady: () => Effect.succeed(true),
  };
  const backendLayer = DesktopBackendPool.layerTest([stubBackendInstance]);

  const desktopWindowLayer = Layer.succeed(DesktopWindow.DesktopWindow, {
    createMain: Effect.die("unexpected createMain"),
    ensureMain: Effect.die("unexpected ensureMain"),
    revealOrCreateMain: Effect.sync(() => {
      revealOrCreateMainCount += 1;
      return {} as Electron.BrowserWindow;
    }),
    activate: Effect.void,
    createMainIfBackendReady: Effect.void,
    showConnectingSplash: Effect.void,
    handleBackendReady: () => Effect.void,
    handleBackendNotReady: Effect.void,
    flushMainWindowBounds: Effect.void,
    dispatchMenuAction: () => Effect.void,
    zoomMain: () => Effect.void,
    syncAppearance: Effect.void,
  } satisfies DesktopWindow.DesktopWindow["Service"]);

  const electronAppLayer = Layer.succeed(ElectronApp.ElectronApp, {
    metadata: Effect.die("unexpected metadata read"),
    name: Effect.succeed("S5 Code"),
    systemLocale: Effect.succeed("en-US"),
    whenReady: Effect.void,
    quit: Effect.sync(() => {
      appQuitCount += 1;
    }),
    exit: () => Effect.void,
    relaunch: () => Effect.void,
    setPath: () => Effect.void,
    setName: () => Effect.void,
    setAboutPanelOptions: () => Effect.void,
    setAppUserModelId: () => Effect.void,
    getAppMetrics: Effect.succeed([]),
    isDefaultProtocolClient: () => Effect.succeed(false),
    setAsDefaultProtocolClient: () => Effect.succeed(true),
    setDesktopName: () => Effect.void,
    setDockIcon: () => Effect.void,
    appendCommandLineSwitch: () => Effect.void,
    onBeforeQuitForUpdate: () => Effect.void,
    removeCommandLineSwitch: () => Effect.void,
    on: () => Effect.void,
  } satisfies ElectronApp.ElectronApp["Service"]);

  const macUnsignedUpdateInstallLayer = Layer.succeed(
    MacUnsignedUpdateInstall.MacUnsignedUpdateInstall,
    {
      usesSquirrelCompatibleSignature: () =>
        Effect.succeed(options.squirrelCompatibleSignature ?? true),
      installDownloadedZip: (input) =>
        Effect.sync(() => {
          bundleReplaceCalls.push(input);
        }).pipe(Effect.andThen(options.unsignedMacInstall ?? Effect.void)),
    } satisfies MacUnsignedUpdateInstall.MacUnsignedUpdateInstall["Service"],
  );

  const environmentLayer = DesktopEnvironment.layer({
    dirname: "/repo/apps/desktop/src",
    homeDirectory: `/tmp/t3-desktop-updates-home-${process.pid}`,
    platform: "darwin",
    processArch: "x64",
    appVersion: "1.2.3",
    appPath: options.appPath ?? "/repo",
    isPackaged: true,
    resourcesPath: "/missing/resources",
    runningUnderArm64Translation: false,
  }).pipe(
    Layer.provide(
      Layer.mergeAll(
        NodeServices.layer,
        DesktopConfig.layerTest({
          T3CODE_HOME: `/tmp/t3-desktop-updates-test-${process.pid}`,
          T3CODE_DESKTOP_MOCK_UPDATES: "true",
          T3CODE_DESKTOP_MOCK_UPDATE_SERVER_PORT: "4141",
          ...options.env,
        }),
      ),
    ),
  );

  let testSettings: DesktopAppSettings.DesktopSettings = {
    ...DesktopAppSettings.DEFAULT_DESKTOP_SETTINGS,
  };
  const setUpdateChannelError = options.setUpdateChannelError;
  const settingsLayer =
    setUpdateChannelError || options.beforeSetUpdateChannel
      ? Layer.succeed(DesktopAppSettings.DesktopAppSettings, {
          get: Effect.sync(() => testSettings),
          load: Effect.sync(() => testSettings),
          setMainWindowBounds: () => Effect.die("unexpected main window bounds update"),
          setServerExposureMode: () => Effect.die("unexpected server exposure update"),
          setTailscaleServe: () => Effect.die("unexpected Tailscale Serve update"),
          setUpdateChannel: (channel) =>
            setUpdateChannelError
              ? Effect.fail(setUpdateChannelError)
              : (options.beforeSetUpdateChannel ?? Effect.void).pipe(
                  Effect.andThen(
                    Effect.sync(() => {
                      const changed = testSettings.updateChannel !== channel;
                      testSettings = {
                        ...testSettings,
                        updateChannel: channel,
                        updateChannelConfiguredByUser: true,
                      };
                      return { settings: testSettings, changed };
                    }),
                  ),
                ),
          setWslBackendEnabled: () => Effect.die("unexpected WSL backend toggle"),
          setWslDistro: () => Effect.die("unexpected WSL distro change"),
          setWslOnly: () => Effect.die("unexpected WSL-only toggle"),
          applyWslWindowsFallback: Effect.die("unexpected WSL Windows fallback"),
          applyWslWindowsFallbackInMemory: Effect.die("unexpected WSL Windows fallback"),
        } satisfies DesktopAppSettings.DesktopAppSettings["Service"])
      : DesktopAppSettings.layer;

  const layer = DesktopUpdates.layer.pipe(
    Layer.provideMerge(updaterLayer),
    Layer.provideMerge(windowLayer),
    Layer.provideMerge(desktopWindowLayer),
    Layer.provideMerge(backendLayer),
    Layer.provideMerge(DesktopState.layer),
    Layer.provideMerge(settingsLayer),
    Layer.provideMerge(electronAppLayer),
    Layer.provideMerge(macUnsignedUpdateInstallLayer),
    Layer.provideMerge(
      DesktopConfig.layerTest({
        T3CODE_HOME: `/tmp/t3-desktop-updates-test-${process.pid}`,
        T3CODE_DESKTOP_MOCK_UPDATES: "true",
        T3CODE_DESKTOP_MOCK_UPDATE_SERVER_PORT: "4141",
        ...options.env,
      }),
    ),
    Layer.provideMerge(environmentLayer),
    Layer.provideMerge(NodeServices.layer),
  );

  return {
    layer,
    checkCount: () => checkCount,
    quitAndInstalls: () => quitAndInstallCount,
    appQuitCount: () => appQuitCount,
    bundleReplaceCalls: () => bundleReplaceCalls,
    backendStartCount: () => backendStartCount,
    destroyAllCount: () => destroyAllCount,
    revealOrCreateMainCount: () => revealOrCreateMainCount,
    installSteps,
    downloadCount: () => downloadCount,
    feedUrls: () => feedUrls,
    fullChangelog: () => fullChangelog,
    listenerCount: () =>
      Array.from(listeners.values()).reduce(
        (total, eventListeners) => total + eventListeners.size,
        0,
      ),
    sentStates,
    emit: (eventName: string, payload?: unknown) => {
      for (const listener of listeners.get(eventName) ?? []) {
        listener(payload);
      }
    },
  };
}
