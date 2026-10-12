// @effect-diagnostics nodeBuiltinImport:off -- Assertions in the standalone Bun stream regression fixture.
import * as NodeAssert from "node:assert/strict";
import * as BunHttpServer from "@effect/platform-bun/BunHttpServer";
import {
  AuthOrchestrationReadScope,
  AuthPreviewOperateScope,
  AuthSessionId,
} from "@t3tools/contracts";
import * as Context from "effect/Context";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Queue from "effect/Queue";
import * as Scope from "effect/Scope";
import { HttpRouter, HttpServer } from "effect/http";

import * as EnvironmentAuth from "../../auth/EnvironmentAuth.ts";
import * as ServerBrowser from "../ServerBrowser.ts";
import { routeLayer } from "../ServerBrowserStream.ts";

// Use the native HTTP adapter shipped by the standalone server, not Node's adapter.
// This fixture runs in its own process. Its HTTP scopes live until that process
// exits, so graceful shutdown does not wait for Bun's upgraded requests to drain.
for (const slow of [false, true]) {
  const scope = Effect.runSync(Scope.make());
  await Effect.runPromise(
    Effect.gen(function* () {
      const output = yield* Queue.make<ServerBrowser.ServerBrowserViewerOutput>();
      const acked = yield* Deferred.make<void>();
      const inputs: unknown[] = [];
      const browser = Layer.mock(ServerBrowser.ServerBrowser, {
        attachViewer: () =>
          Effect.succeed({
            output,
            input: (message) => Effect.sync(() => void inputs.push(message)),
          }),
      });
      const auth = Layer.mock(EnvironmentAuth.EnvironmentAuth, {
        authenticateWebSocketUpgrade: (request) => {
          NodeAssert.equal(
            new URL(request.originalUrl, "http://fixture").searchParams.get("wsTicket"),
            "fixture-ticket",
          );
          return Effect.succeed({
            sessionId: AuthSessionId.make("stream-fixture"),
            subject: "stream-fixture",
            method: "bearer-access-token",
            scopes: [AuthOrchestrationReadScope, AuthPreviewOperateScope],
          });
        },
      });
      const services = yield* Layer.build(
        HttpRouter.serve(routeLayer.pipe(Layer.provide(browser)), {
          disableListenLog: true,
          disableLogger: true,
        }).pipe(
          Layer.provideMerge(
            BunHttpServer.layer({
              hostname: "127.0.0.1",
              port: 0,
              websocket: { perMessageDeflate: true },
            }),
          ),
          Layer.provide(auth),
        ),
      );
      const server = Context.get(services, HttpServer.HttpServer);
      const origin = HttpServer.formatAddress(server.address).replace(/^http/, "ws");
      const frame = new Uint8Array([255, 216, 255, 217]);
      yield* Queue.offer(output, { _tag: "viewport", width: 1280, height: 800 });
      yield* Queue.offer(output, {
        _tag: "control",
        canOperate: true,
        controller: "you",
        generation: 1,
        dialog: null,
      });
      for (let index = 0; index < (slow ? 65 : 1); index++) {
        yield* Queue.offer(output, {
          _tag: "frame",
          data: frame,
          ack: Deferred.succeed(acked, undefined).pipe(Effect.asVoid),
        });
      }
      const received = Promise.withResolvers<void>();
      const closed = Promise.withResolvers<number>();
      const messages: unknown[] = [];
      const frames: Uint8Array[] = [];
      const socket = yield* Effect.acquireRelease(
        Effect.sync(() => {
          const socket = new WebSocket(
            `${origin}/api/preview-stream/ws?threadId=thread&tabId=tab&wsTicket=fixture-ticket`,
          );
          socket.binaryType = "arraybuffer";
          socket.addEventListener("error", () => received.reject(new Error("stream failed")));
          socket.addEventListener("close", (event) => {
            closed.resolve(event.code);
            if (!slow) received.reject(new Error(`stream closed: ${event.code}`));
          });
          socket.addEventListener("message", (event) => {
            if (typeof event.data === "string") {
              messages.push(JSON.parse(event.data));
              return;
            }
            frames.push(new Uint8Array(event.data));
            if (!slow) {
              socket.send(JSON.stringify({ type: "reload" }));
              socket.send(JSON.stringify({ type: "ack" }));
              received.resolve();
            }
          });
          return socket;
        }),
        (socket) => Effect.sync(() => socket.close()),
      );
      if (slow) {
        NodeAssert.equal(yield* Effect.promise(() => closed.promise), 1013);
        NodeAssert.equal(frames.length, 64);
      } else {
        yield* Effect.promise(() => received.promise);
        yield* Deferred.await(acked);
        NodeAssert.deepEqual(inputs, [{ type: "reload" }]);
        NodeAssert.deepEqual(frames, [frame]);
        socket.close();
        yield* Effect.promise(() => closed.promise);
      }
      NodeAssert.deepEqual(messages, [
        { type: "viewport", width: 1280, height: 800 },
        { type: "control", canOperate: true, controller: "you", generation: 1, dialog: null },
      ]);
    }).pipe(Effect.provideService(Scope.Scope, scope), Effect.timeout("10 seconds")),
  );
}
console.log(
  "Standalone Bun preview streamed frames and control, handled input and ACKs, and closed a slow viewer.",
);
