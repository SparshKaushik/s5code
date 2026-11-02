// @effect-diagnostics nodeBuiltinImport:off -- The fixture is a real loopback HTTP server, like the one the helper probes.
import * as NodeHttp from "node:http";
import type * as NodeNet from "node:net";

import * as NodeServices from "@effect/platform-node/NodeServices";
import { assert, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import { Command } from "effect/cli";

import { sshHelperCommand } from "./sshHelper.ts";

// Like the standalone server binary, which ships no web client: `/` is unavailable.
const serverWithoutWebClient = Effect.acquireRelease(
  Effect.promise(
    () =>
      new Promise<NodeHttp.Server>((resolve) => {
        const server = NodeHttp.createServer((request, response) => {
          response.writeHead(request.url === "/.well-known/t3/environment" ? 200 : 503).end();
        });
        server.listen(0, "127.0.0.1", () => resolve(server));
      }),
  ),
  (server) => Effect.promise(() => new Promise<void>((resolve) => server.close(() => resolve()))),
);

it.live("wait-ready accepts a running server that ships no web client", () =>
  Effect.gen(function* () {
    const server = yield* serverWithoutWebClient;
    const { port } = server.address() as NodeNet.AddressInfo;
    // The helper reports failure through process.exitCode, which outlives this test.
    const exitCodeBefore = process.exitCode;
    const exitCode = yield* Command.runWith(sshHelperCommand, { version: "0.0.0" })([
      "wait-ready",
      String(port),
      "1000",
      "200",
    ]).pipe(
      Effect.andThen(Effect.sync(() => process.exitCode)),
      Effect.ensuring(
        Effect.sync(() => {
          process.exitCode = exitCodeBefore;
        }),
      ),
    );
    assert.equal(exitCode, exitCodeBefore);
  }).pipe(Effect.provide(NodeServices.layer)),
);
