import { DeviceHostId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";
import * as Clock from "effect/Clock";
import * as Schedule from "effect/Schedule";
import * as Schema from "effect/Schema";
import { HttpClient, HttpClientRequest, HttpClientResponse } from "effect/http";
import * as DeviceHost from "./DeviceHost.ts";

const tunnel = Schema.String.check(
  Schema.makeFilter((value) => {
    try {
      const url = new URL(value);
      return (
        url.protocol === "https:" &&
        /^[a-z0-9-]+\.trycloudflare\.com$/.test(url.hostname) &&
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
const token = Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(4096));
const isHostError = Schema.is(DeviceHost.DeviceHostError);
export const Registration = Schema.Struct({
  version: Schema.Literal(1),
  createdAt: Schema.Number,
  runId: Schema.String.check(
    Schema.isPattern(/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/),
  ),
  tunnelUrl: tunnel,
  daemonToken: token,
  expiresAt: Schema.NullOr(Schema.Number),
  platform: Schema.optional(Schema.Literals(["ios", "android"])),
  deviceId: Schema.optional(Schema.String.check(Schema.isMaxLength(128))),
  deviceName: Schema.optional(Schema.String.check(Schema.isMaxLength(150))),
  agentSession: Schema.optional(
    Schema.String.check(Schema.isMinLength(1), Schema.isMaxLength(256)),
  ),
  stop: Schema.Boolean,
  autoBoot: Schema.Boolean,
  apiUrl: Schema.Literal("https://api.simbox.touchtech.club"),
  userToken: Schema.NullOr(token),
});
export type Registration = typeof Registration.Type;

const CurrentRun = Schema.Struct({
  run: Schema.NullOr(
    Schema.Struct({
      id: Schema.String,
      state: Schema.String,
      tunnelUrl: Schema.NullOr(tunnel),
      daemonToken: Schema.NullOr(token),
      expiresAt: Schema.NullOr(Schema.Number),
    }),
  ),
});

export const hostId = (runId: string) => DeviceHostId.make(`simbox-${runId}`);

/** A Simbox runner owns its tools. S5 connects to it without installing a second hub. */
export const make = Effect.fn("SimboxDeviceHost.make")(function* (input: Registration) {
  const http = (yield* HttpClient.HttpClient).pipe(HttpClient.withScope);
  const id = hostId(input.runId);
  let connection = input;
  let stopped = false;
  const hub: DeviceHost.DeviceHubEndpoint = {
    get origin() {
      return `${connection.tunnelUrl.replace(/\/$/, "")}/simbox-device-hub`;
    },
    get headers() {
      return { authorization: `Bearer ${connection.daemonToken}` };
    },
  };
  const ready: DeviceHost.DeviceHostAgentReady = {
    supportsActions: false,
    nodePath: "",
    hub,
    helpers: { serveSimAxSettings: null, serveSimCli: null },
    agentDevice: {
      get baseUrl() {
        return `${connection.tunnelUrl.replace(/\/$/, "")}/agent-device`;
      },
      get token() {
        return connection.daemonToken;
      },
      entryPath: "",
    },
    run: (command, args) =>
      Effect.succeed(
        command === "emulator" && args.length === 1 && args[0] === "-list-avds"
          ? { code: 0, stdout: "", stderr: "" }
          : { code: 1, stdout: "", stderr: "Host settings are unavailable on this Simbox runner." },
      ),
  };
  const refresh = Effect.gen(function* () {
    const now = yield* Clock.currentTimeMillis;
    if (stopped || (connection.expiresAt !== null && connection.expiresAt * 1000 <= now))
      return false;
    if (!connection.userToken) return true;
    const latest = yield* http
      .execute(
        HttpClientRequest.get(`${connection.apiUrl}/v1/runs/current`).pipe(
          HttpClientRequest.setHeader("authorization", `Bearer ${connection.userToken}`),
        ),
      )
      .pipe(
        Effect.flatMap(HttpClientResponse.filterStatusOk),
        Effect.flatMap(HttpClientResponse.schemaBodyJson(CurrentRun)),
        Effect.scoped,
        Effect.timeout("10 seconds"),
        Effect.mapError(
          (cause) =>
            new DeviceHost.DeviceHostError({
              hostId: id,
              step: "refreshing Simbox connection",
              cause,
            }),
        ),
      );
    if (
      !latest.run ||
      latest.run.id !== input.runId ||
      !["live", "closing"].includes(latest.run.state)
    )
      return false;
    if (latest.run.state === "closing") return false;
    if (latest.run.tunnelUrl && latest.run.daemonToken)
      connection = {
        ...connection,
        tunnelUrl: latest.run.tunnelUrl,
        daemonToken: latest.run.daemonToken,
        expiresAt: latest.run.expiresAt,
      };
    return true;
  });
  const ensureReady = Effect.gen(function* () {
    if (!(yield* refresh))
      return yield* new DeviceHost.DeviceHostError({
        hostId: id,
        step: "connecting to an expired Simbox run",
        cause: new Error("Run ended"),
      });
    yield* http
      .execute(
        HttpClientRequest.get(`${hub.origin}/api/devices`).pipe(
          HttpClientRequest.setHeaders(hub.headers!),
        ),
      )
      .pipe(
        Effect.flatMap((response) =>
          Effect.gen(function* () {
            if (response.status === 503)
              return yield* new DeviceHost.DeviceHostError({
                hostId: id,
                step: "starting Simbox device hub",
                cause: new Error("Hub starting"),
              });
            yield* HttpClientResponse.filterStatusOk(response);
          }),
        ),
        Effect.scoped,
        Effect.timeout("15 seconds"),
        Effect.mapError((cause) =>
          isHostError(cause)
            ? cause
            : new DeviceHost.DeviceHostError({
                hostId: id,
                step: "connecting to Simbox device hub",
                cause,
              }),
        ),
        Effect.retry({
          schedule: Schedule.spaced("2 seconds"),
          times: 120,
          while: (error) => error.step === "starting Simbox device hub",
        }),
      );
    return ready;
  });
  const host: DeviceHost.DeviceHost["Service"] = {
    id,
    summary: Effect.succeed({
      id,
      kind: "simbox",
      label: `Simbox ${input.runId.slice(0, 8)}`,
      platforms: ["ios", "android"].map((platform) => ({
        platform: platform as "ios" | "android",
        available: !input.platform || input.platform === platform,
      })),
      hubInstalled: true,
      agentDeviceInstalled: true,
    }),
    platformAvailability: (platform) =>
      Effect.succeed({ platform, available: !input.platform || input.platform === platform }),
    current: Effect.sync(() => (stopped ? null : ready)),
    ensureReady: (onPhase) =>
      onPhase("starting", "Connecting to Simbox…").pipe(Effect.andThen(ensureReady)),
    ensureAgentReady: (onPhase) =>
      onPhase("starting", "Connecting to Simbox…").pipe(Effect.andThen(ensureReady)),
    stopAgent: Effect.void,
    stop: Effect.sync(() => {
      stopped = true;
    }),
  };
  return {
    host,
    ready,
    get agentSession() {
      return connection.agentSession ?? id;
    },
    refresh,
    update: (next: Registration) =>
      Effect.sync(() => {
        connection = next;
      }),
  };
});
