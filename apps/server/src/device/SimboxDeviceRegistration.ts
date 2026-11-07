/**
 * S5: a Simbox run is a standalone Simlock daemon behind a per-run tunnel. The simbox CLI leaves a
 * private receipt when an agent connects to one; this saves the run as a Simlock host and opens the
 * agent's leased device in its thread. Runs whose tunnel stays gone are removed again.
 */
import {
  DeviceHostId,
  type OrchestrationV2TurnItem,
  SimlockDeviceHostConfig,
  type ThreadId,
} from "@t3tools/contracts";
import * as Clock from "effect/Clock";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import * as PubSub from "effect/PubSub";
import * as Schema from "effect/Schema";
import * as Semaphore from "effect/Semaphore";
import { HttpClient, HttpClientRequest, HttpClientResponse } from "effect/http";
import * as HostProcess from "@t3tools/shared/HostProcess";
import * as ServerSettings from "../serverSettings.ts";
import * as DeviceService from "./DeviceService.ts";

const sameHosts = Schema.toEquivalence(Schema.Array(SimlockDeviceHostConfig));

const receiptPattern =
  /\[simbox-device:([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})\]/g;
const HOST_PREFIX = "simbox-";
// A run's tunnel is deleted when it ends; brief outages recover well within this.
const ENDED_AFTER_MS = 5 * 60_000;

const tunnel = Schema.String.check(
  Schema.makeFilter((value) => {
    try {
      const url = new URL(value);
      return (
        url.protocol === "https:" &&
        /^r-[0-9a-f]{16}\.(simboxtouchtech|simbox)\.dpdns\.org$/.test(url.hostname) &&
        !url.port &&
        !url.username &&
        !url.password &&
        url.pathname === "/" &&
        !url.search &&
        !url.hash
      );
    } catch {
      return false;
    }
  }),
);

export const Registration = Schema.Struct({
  version: Schema.Literal(1),
  createdAt: Schema.Number,
  runId: Schema.String.check(
    Schema.isPattern(/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/),
  ),
  tunnelUrl: tunnel,
  daemonToken: Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(4096)),
  platform: Schema.optional(Schema.Literals(["ios", "android"])),
  stop: Schema.Boolean,
});
export type Registration = typeof Registration.Type;

export const hostConfig = (input: Registration): SimlockDeviceHostConfig => ({
  kind: "simlock",
  id: DeviceHostId.make(`${HOST_PREFIX}${input.runId}`),
  label: `Simbox ${input.platform === "android" ? "Android" : input.platform === "ios" ? "iOS" : "run"} ${input.runId.slice(0, 8)}`,
  gatewayUrl: input.tunnelUrl,
  agentToken: input.daemonToken,
  maxLeases: 2,
  maxLeaseMinutes: 60,
});

/** Read only tool output, never prompts, tool arguments, or assistant prose. */
export function receiptsForItem(
  item: Pick<OrchestrationV2TurnItem, "type"> & { readonly output?: unknown },
): string[] {
  if (item.type !== "command_execution" && item.type !== "dynamic_tool") return [];
  const ids = new Set<string>();
  const visit = (value: unknown, depth = 0) => {
    if (typeof value === "string") {
      for (const match of value.slice(-2_000_000).matchAll(receiptPattern)) ids.add(match[1]!);
    } else if (depth < 4 && value && typeof value === "object") {
      for (const child of Object.values(value).slice(0, 128)) visit(child, depth + 1);
    }
  };
  visit(item.output);
  return [...ids].slice(0, 16);
}

export class SimboxDeviceRegistrationError extends Schema.TaggedError<SimboxDeviceRegistrationError>()(
  "SimboxDeviceRegistrationError",
  {
    step: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Simbox device attachment failed while ${this.step}.`;
  }
}

export class SimboxDeviceRegistration extends Context.Service<
  SimboxDeviceRegistration,
  {
    readonly observe: (
      item: OrchestrationV2TurnItem,
    ) => Effect.Effect<void, SimboxDeviceRegistrationError>;
  }
>()("t3/device/SimboxDeviceRegistration") {}

const make = Effect.gen(function* () {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const http = yield* HttpClient.HttpClient;
  const devices = yield* DeviceService.DeviceService;
  const settings = yield* ServerSettings.ServerSettingsService;
  const receiptLock = yield* Semaphore.make(1);
  const settingsLock = yield* Semaphore.make(1);
  const environment = yield* HostProcess.Environment;
  const home = yield* HostProcess.HomeDirectory;
  const directory =
    environment.SIMBOX_DEVICE_REGISTRATION_DIR ??
    path.join(
      environment.XDG_CONFIG_HOME ?? path.join(home, ".config"),
      "simbox",
      "device-registrations",
    );
  const seen = new Set<string>();
  const decode = Schema.decodeUnknownEffect(Schema.fromJsonString(Registration));

  const claim = (id: string) =>
    receiptLock.withPermit(
      Effect.gen(function* () {
        if (seen.has(id)) return null;
        seen.add(id);
        if (seen.size > 4096) seen.delete(seen.values().next().value!);
        const file = path.join(directory, `${id}.json`);
        // The file must be a fresh CLI receipt; arbitrary output is insufficient.
        if (!(yield* fs.exists(file))) return null;
        if (Number((yield* fs.stat(file)).size) > 16_384) return null;
        const claimed = path.join(directory, `${id}.claimed`);
        yield* fs.rename(file, claimed);
        return yield* Effect.gen(function* () {
          const input = yield* decode(yield* fs.readFileString(claimed));
          const age = (yield* Clock.currentTimeMillis) - input.createdAt;
          return age < -10_000 || age > 5 * 60_000 ? null : input;
        }).pipe(Effect.ensuring(fs.remove(claimed, { force: true }).pipe(Effect.ignore)));
      }),
    );

  const updateHosts = (
    change: (
      hosts: ReadonlyArray<SimlockDeviceHostConfig>,
    ) => ReadonlyArray<SimlockDeviceHostConfig>,
  ) =>
    settingsLock.withPermit(
      Effect.gen(function* () {
        const current = yield* settings.getSettings;
        const next = change(current.simlockDeviceHosts);
        if (sameHosts(next, current.simlockDeviceHosts)) return;
        yield* settings.updateSettings({ simlockDeviceHosts: next });
      }),
    );

  const waitForHost = (hostId: string) =>
    Effect.scoped(
      Effect.gen(function* () {
        const changes = yield* devices.subscribe;
        let state = yield* devices.state;
        while (!state.hosts.some((host) => host.id === hostId)) state = yield* PubSub.take(changes);
      }),
    ).pipe(Effect.timeout("30 seconds"));

  const attach = Effect.fn("SimboxDeviceRegistration.attach")(function* (
    threadId: ThreadId,
    input: Registration,
  ) {
    const config = hostConfig(input);
    if (input.stop) {
      yield* updateHosts((hosts) => hosts.filter((host) => host.id !== config.id));
      return;
    }
    const consent = yield* settings.getSettings;
    if (!consent.enableDeviceSupport || !consent.enableAgentDeviceAccess) {
      yield* Effect.logInfo(
        "Enable device support and agent device access to watch Simbox devices in S5.",
      );
      return;
    }
    yield* updateHosts((hosts) => [...hosts.filter((host) => host.id !== config.id), config]);
    if (!consent.simlockDeviceHostsEnabled || !consent.deviceOnboardingCompleted)
      yield* settings.updateSettings({
        simlockDeviceHostsEnabled: true,
        deviceOnboardingCompleted: true,
      });
    yield* waitForHost(config.id);
    const open = yield* devices.sessionsForThread(threadId);
    if (open.some((session) => session.hostId === config.id)) return;
    // A listed, booted device on a Simbox host is a lease: the agent's own, or one opened here.
    const listed = yield* devices.list;
    yield* Effect.forEach(
      listed.devices.filter(
        (device) =>
          device.hostId === config.id &&
          device.booted &&
          (!input.platform || device.platform === input.platform),
      ),
      (device) =>
        devices.open({
          threadId,
          hostId: config.id,
          deviceId: device.id,
          platform: device.platform,
          boot: false,
        }),
      { discard: true },
    );
  });

  const observe: SimboxDeviceRegistration["Service"]["observe"] = Effect.fn(
    "SimboxDeviceRegistration.observe",
  )(function* (item) {
    for (const id of receiptsForItem(item)) {
      const input = yield* claim(id).pipe(
        Effect.mapError(
          (cause) => new SimboxDeviceRegistrationError({ step: "reading the CLI receipt", cause }),
        ),
      );
      if (input)
        yield* attach(item.threadId, input).pipe(
          Effect.mapError(
            (cause) => new SimboxDeviceRegistrationError({ step: "attaching the run", cause }),
          ),
        );
    }
  });

  const failingSince = new Map<string, number>();
  const alive = (gatewayUrl: string) =>
    http.execute(HttpClientRequest.get(`${gatewayUrl.replace(/\/$/, "")}/v1/healthz`)).pipe(
      Effect.flatMap(HttpClientResponse.filterStatusOk),
      Effect.scoped,
      Effect.timeout("15 seconds"),
      Effect.as(true),
      Effect.orElseSucceed(() => false),
    );
  const removeEndedRuns = Effect.gen(function* () {
    const runs = (yield* settings.getSettings).simlockDeviceHosts.filter((host) =>
      host.id.startsWith(HOST_PREFIX),
    );
    const now = yield* Clock.currentTimeMillis;
    const ended = new Set<string>();
    for (const id of failingSince.keys())
      if (!runs.some((run) => run.id === id)) failingSince.delete(id);
    yield* Effect.forEach(
      runs,
      (run) =>
        alive(run.gatewayUrl).pipe(
          Effect.map((up) => {
            if (up) return failingSince.delete(run.id);
            const since = failingSince.get(run.id) ?? now;
            failingSince.set(run.id, since);
            if (now - since >= ENDED_AFTER_MS) ended.add(run.id);
          }),
        ),
      { discard: true, concurrency: 4 },
    );
    if (ended.size === 0) return;
    yield* updateHosts((hosts) => hosts.filter((host) => !ended.has(host.id)));
    for (const id of ended) failingSince.delete(id);
  });
  yield* removeEndedRuns.pipe(
    Effect.catch((error) => Effect.logWarning("Could not check Simbox runs", { error })),
    Effect.andThen(Effect.sleep("1 minute")),
    Effect.forever,
    Effect.forkScoped,
  );

  return SimboxDeviceRegistration.of({ observe });
});

export const layer = Layer.effect(SimboxDeviceRegistration, make);
