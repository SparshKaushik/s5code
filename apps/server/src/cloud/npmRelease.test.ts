import { expect, it } from "@effect/vitest";
import { ServerNpmReleaseError } from "@t3tools/contracts";
import { cliReleaseIndexPageUrl, cliReleaseTagUrl } from "@t3tools/shared/cliRelease";
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import { HttpClient, HttpClientResponse } from "effect/http";
import * as Result from "effect/Result";
import * as TestClock from "effect/testing/TestClock";

import * as NpmRelease from "./npmRelease.ts";

const stable = "0.2.62";
const nightly = "0.2.63-nightly.20261103.1";
const releaseIndex = [
  { tag_name: "v0.2.64", draft: true },
  { tag_name: `v${nightly}` },
  { tag_name: `v${stable}` },
];

const makeService = (respond: (url: string) => Response | "stall") => {
  const urls: string[] = [];
  return NpmRelease.make().pipe(
    Effect.provideService(
      HttpClient.HttpClient,
      HttpClient.make((request) => {
        urls.push(request.url);
        const response = respond(request.url);
        return response === "stall"
          ? Effect.never
          : Effect.succeed(HttpClientResponse.fromWeb(request, response));
      }),
    ),
    Effect.map((service) => ({ service, urls })),
  );
};

const github = (url: string) => {
  if (url === cliReleaseIndexPageUrl(1)) return Response.json(releaseIndex);
  if (url === cliReleaseTagUrl(stable)) return Response.json({ tag_name: `v${stable}` });
  if (url === cliReleaseTagUrl("0.2.64"))
    return Response.json({ tag_name: "v0.2.64", draft: true });
  return new Response("{}", { status: 404 });
};

it.effect.each([
  { spec: "latest", url: cliReleaseIndexPageUrl(1), version: stable },
  { spec: "nightly", url: cliReleaseIndexPageUrl(1), version: nightly },
  { spec: stable, url: cliReleaseTagUrl(stable), version: stable },
])("reads $spec from the fork's GitHub Releases and caches it for a minute", (fixture) =>
  Effect.gen(function* () {
    const { service, urls } = yield* makeService(github);
    expect(yield* service.get({ spec: fixture.spec })).toEqual({ version: fixture.version });
    expect(yield* service.get({ spec: fixture.spec })).toEqual({ version: fixture.version });
    expect(urls).toEqual([fixture.url]);
    yield* TestClock.adjust("1 minute");
    yield* service.get({ spec: fixture.spec });
    expect(urls).toHaveLength(2);
  }),
);

it.effect.each(["9.9.9", "0.2.64"])("reports %s as unpublished", (spec) =>
  Effect.gen(function* () {
    const { service } = yield* makeService(github);
    expect(yield* service.get({ spec })).toBeNull();
  }),
);

it.effect.each([
  { name: "a server error", response: () => new Response("{}", { status: 503 }) },
  { name: "an unexpected body", response: () => Response.json({ tag_name: 42 }) },
  { name: "a non-array index", response: () => Response.json({ message: "rate limited" }) },
])("reports an unavailable lookup rather than an unpublished release on $name", (fixture) =>
  Effect.gen(function* () {
    const { service } = yield* makeService(fixture.response);
    for (const spec of [stable, "latest"]) {
      const found = yield* service.get({ spec }).pipe(Effect.result);
      expect(Result.isFailure(found)).toBe(true);
      if (Result.isFailure(found)) expect(found.failure).toBeInstanceOf(ServerNpmReleaseError);
    }
  }),
);

it.effect("fails a lookup GitHub never answers after 15 seconds", () =>
  Effect.gen(function* () {
    const { service } = yield* makeService(() => "stall");
    const stalled = yield* service.get({ spec: "latest" }).pipe(Effect.result, Effect.forkChild);
    yield* TestClock.adjust("15 seconds");
    expect(Result.isFailure(yield* Fiber.join(stalled))).toBe(true);
  }),
);

it.effect.each(["--registry=https://example.com", "../other", "other-package@latest", "*", ""])(
  "rejects invalid release specs before any request: %s",
  (spec) =>
    Effect.gen(function* () {
      const { service, urls } = yield* makeService(github);
      expect(Result.isFailure(yield* service.get({ spec }).pipe(Effect.result))).toBe(true);
      expect(urls).toEqual([]);
    }),
);
