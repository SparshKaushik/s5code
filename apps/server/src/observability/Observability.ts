import * as HttpObservability from "@t3tools/shared/httpObservability";
import { makeLocalFileTracer, makeTraceSink } from "@t3tools/shared/observability";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as References from "effect/References";
import * as Tracer from "effect/Tracer";
import * as Statement from "effect/sql/Statement";

import * as ServerConfig from "../config.ts";
import * as ResourceAttribution from "../resourceTelemetry/ResourceAttribution.ts";
import * as ServerLogger from "../serverLogger.ts";
import * as HeapAttribution from "./HeapAttribution.ts";

export const layer = Layer.unwrap(
  Effect.gen(function* () {
    const config = yield* ServerConfig.ServerConfig;
    const attribution = yield* ResourceAttribution.ResourceAttribution;

    const layerTraceReferences = Layer.mergeAll(
      Layer.succeed(Tracer.MinimumTraceLevel, config.traceMinLevel),
      Layer.succeed(References.TracerTimingEnabled, config.traceTimingEnabled),
      // Preserve SQL spans while awaiting connections so lock waits can be recorded on them.
      Layer.succeed(Statement.SpanPropagationEnabled, true),
      HttpObservability.layer,
    );

    const layerTracer = Layer.unwrap(
      Effect.gen(function* () {
        const sink = yield* makeTraceSink({
          filePath: config.serverTracePath,
          maxBytes: config.traceMaxBytes,
          maxFiles: config.traceMaxFiles,
          batchWindowMs: config.traceBatchWindowMs,
          onFlush: (stats) =>
            attribution.record({
              component: "server-trace",
              operation: "append",
              logicalWriteBytes: stats.logicalWriteBytes,
              count: stats.count,
              durationMs: stats.durationMs,
            }),
        });

        const tracer = yield* makeLocalFileTracer({
          filePath: config.serverTracePath,
          maxBytes: config.traceMaxBytes,
          maxFiles: config.traceMaxFiles,
          batchWindowMs: config.traceBatchWindowMs,
          sink,
        });

        // Bun's getHeapStatistics walks the heap (about 10 ms a call, twice per span), which
        // stalled the standalone server for minutes, so its binary traces without attribution.
        if (typeof Bun !== "undefined") {
          return Layer.succeed(Tracer.Tracer, tracer);
        }
        const heapAttribution = yield* HeapAttribution.make(tracer);
        yield* heapAttribution.run;

        return Layer.succeed(Tracer.Tracer, heapAttribution.tracer);
      }),
    );

    return Layer.mergeAll(ServerLogger.layer, layerTraceReferences, layerTracer);
  }),
);
