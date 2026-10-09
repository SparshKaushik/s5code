import { describe, expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as Deferred from "effect/Deferred";
import * as Fiber from "effect/Fiber";
import * as Schema from "effect/Schema";
import { HttpClient, HttpClientResponse } from "effect/http";
import { TestClock } from "effect/testing";
import * as SimboxHost from "./SimboxDeviceHost.ts";

const registration = (): SimboxHost.Registration => ({
  version: 1,
  createdAt: 0,
  runId: "039dd761-9144-4a9c-a03b-e44c893d73ad",
  tunnelUrl: "https://initial.trycloudflare.com",
  daemonToken: "initial-private-token",
  expiresAt: null,
  platform: "ios",
  stop: false,
  autoBoot: true,
  apiUrl: "https://api.simbox.touchtech.club",
  userToken: "private-app-token",
});

describe("Simbox device host", () => {
  it.effect("retries a warming hub and returns ready once discovery becomes available", () =>
    Effect.gen(function* () {
      const firstAttempt = yield* Deferred.make<void>();
      let attempts = 0;
      const client = HttpClient.make((request) =>
        Effect.gen(function* () {
          attempts++;
          if (attempts === 1) yield* Deferred.succeed(firstAttempt, undefined);
          return HttpClientResponse.fromWeb(
            request,
            new Response("", { status: attempts === 1 ? 503 : 200 }),
          );
        }),
      );
      const remote = yield* SimboxHost.make({ ...registration(), userToken: null }).pipe(
        Effect.provideService(HttpClient.HttpClient, client),
      );
      const connecting = yield* remote.host.ensureReady(() => Effect.void).pipe(Effect.forkChild);
      yield* Deferred.await(firstAttempt);
      yield* TestClock.adjust("2 seconds");
      expect((yield* Fiber.join(connecting)).hub.origin).toBe(
        "https://initial.trycloudflare.com/simbox-device-hub",
      );
      expect(attempts).toBe(2);
    }),
  );
  it.effect(
    "authenticates the runner, refreshes all connection targets, and detects ended/replaced runs",
    () =>
      Effect.gen(function* () {
        const input = registration();
        let run = {
          id: input.runId,
          state: "live",
          tunnelUrl: input.tunnelUrl,
          daemonToken: input.daemonToken,
          expiresAt: null,
        };
        const requests: Array<{ url: string; auth: string | undefined }> = [];
        const client = HttpClient.make((request) =>
          Effect.sync(() => {
            requests.push({ url: request.url, auth: request.headers.authorization });
            return HttpClientResponse.fromWeb(
              request,
              Response.json(
                request.url.endsWith("/v1/runs/current")
                  ? { run }
                  : { simulators: [], emulators: [] },
              ),
            );
          }),
        );
        const remote = yield* SimboxHost.make(input).pipe(
          Effect.provideService(HttpClient.HttpClient, client),
        );
        const ready = yield* remote.host.ensureAgentReady(() => Effect.void);
        expect(requests).toContainEqual({
          url: "https://api.simbox.touchtech.club/v1/runs/current",
          auth: "Bearer private-app-token",
        });
        expect(requests).toContainEqual({
          url: "https://initial.trycloudflare.com/simbox-device-hub/api/devices",
          auth: "Bearer initial-private-token",
        });
        run = {
          ...run,
          tunnelUrl: "https://recovered.trycloudflare.com",
          daemonToken: "rotated-private-token",
        };
        expect(yield* remote.refresh).toBe(true);
        expect(ready.hub.origin).toBe("https://recovered.trycloudflare.com/simbox-device-hub");
        expect(ready.hub.headers?.authorization).toBe("Bearer rotated-private-token");
        expect(ready.agentDevice.baseUrl).toBe("https://recovered.trycloudflare.com/agent-device");
        expect(ready.agentDevice.token).toBe("rotated-private-token");
        run = { ...run, state: "closing" };
        expect(yield* remote.refresh).toBe(false);
        run = { ...run, state: "live", id: "another-run" };
        expect(yield* remote.refresh).toBe(false);
        yield* remote.host.stop;
        expect(yield* remote.host.current).toBeNull();
      }),
  );

  it.effect(
    "expires a runner without network access and updates a stable host from later CLI receipts",
    () =>
      Effect.gen(function* () {
        const input = { ...registration(), userToken: null };
        const client = HttpClient.make(() =>
          Effect.die("Expired runs must not make network requests"),
        );
        const remote = yield* SimboxHost.make(input).pipe(
          Effect.provideService(HttpClient.HttpClient, client),
        );
        yield* remote.update({ ...input, tunnelUrl: "https://new.trycloudflare.com" });
        expect(remote.ready.hub.origin).toBe("https://new.trycloudflare.com/simbox-device-hub");
        yield* remote.update({ ...input, expiresAt: -1 });
        expect(yield* remote.refresh).toBe(false);
      }),
  );

  it("rejects arbitrary tunnel and API origins before any connection", () => {
    const decode = Schema.decodeUnknownSync(SimboxHost.Registration);
    for (const tunnelUrl of [
      "https://localhost",
      "http://runner.trycloudflare.com",
      "https://runner.trycloudflare.com/private",
      "https://user:password@runner.trycloudflare.com",
      "https://runner.trycloudflare.com.evil.example",
    ])
      expect(() => decode({ ...registration(), tunnelUrl })).toThrow();
    expect(() => decode({ ...registration(), apiUrl: "https://evil.example" })).toThrow();
    expect(() => decode({ ...registration(), runId: "-".repeat(36) })).toThrow();
  });
});
