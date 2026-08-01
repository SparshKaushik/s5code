/**
 * PiRpcConnection — JSONL transport over a spawned `pi --mode rpc` process.
 *
 * pi speaks strict JSONL with LF as the *only* record delimiter. Generic line
 * readers are not protocol-compliant: Node's `readline` and most
 * `splitLines`-style helpers also break on `\r`, `U+2028`, and `U+2029`, all of
 * which are legal inside a JSON string. So we buffer manually, split on `\n`,
 * and strip one optional trailing `\r`.
 *
 * Three kinds of line arrive on stdout and are routed here, not in the adapter:
 *   - `type: "response"` settles the pending command that owns its `id`.
 *   - `type: "extension_ui_request"` is a call *into* us from a pi extension.
 *     Dialog methods block the extension until we answer with a matching
 *     `extension_ui_response`; fire-and-forget methods just inform.
 *   - anything else is an agent event and goes on the event stream.
 *
 * Commands are correlated by a monotonic id we mint, because pi echoes `id`
 * back on the response. `prompt` is the one asymmetric case: its response only
 * confirms *acceptance*, and the actual work reports through events, so callers
 * must not treat the response as turn completion.
 *
 * @module provider/pi/PiRpcConnection
 */
import * as Cause from "effect/Cause";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Queue from "effect/Queue";
import * as Ref from "effect/Ref";
import * as Schema from "effect/Schema";
import * as Scope from "effect/Scope";
import * as Stream from "effect/Stream";
import { ChildProcess, ChildProcessSpawner } from "effect/unstable/process";

import { resolveSpawnCommand } from "@t3tools/shared/shell";

import {
  PiCommandError,
  PiDecodeError,
  PiProcessExitedError,
  PiSpawnError,
  PiTransportError,
  type PiRpcError,
} from "./PiRpcErrors.ts";
import {
  PiExtensionUiRequest,
  PiAgentEvent,
  PiRpcResponseEnvelope,
  type PiRpcIncoming,
} from "./PiRpcSchemas.ts";

const decodeJsonLine = Schema.decodeUnknownEffect(Schema.UnknownFromJsonString);
const encodeJsonLine = Schema.encodeUnknownEffect(Schema.UnknownFromJsonString);
const decodeResponseEnvelope = Schema.decodeUnknownEffect(PiRpcResponseEnvelope);
const decodeExtensionUiRequest = Schema.decodeUnknownEffect(PiExtensionUiRequest);
const decodeAgentEvent = Schema.decodeUnknownEffect(PiAgentEvent);

/** Max stderr bytes retained for diagnostics on unexpected exit. */
const STDERR_TAIL_LIMIT = 4_000;

export interface PiRpcSpawnInput {
  readonly command: string;
  readonly args: ReadonlyArray<string>;
  readonly cwd: string;
  readonly env?: NodeJS.ProcessEnv | undefined;
}

export interface PiRpcLogEvent {
  readonly direction: "incoming" | "outgoing";
  readonly payload: unknown;
}

export interface PiRpcConnectionOptions {
  readonly spawn: PiRpcSpawnInput;
  readonly logger?: ((event: PiRpcLogEvent) => Effect.Effect<void>) | undefined;
}

export interface PiRpcConnection {
  readonly pid: number;
  /**
   * Send a command and await its response. Fails with `PiCommandError` when pi
   * reports `success: false`, so callers can pattern-match a rejected command
   * apart from a broken transport.
   */
  readonly request: (
    command: string,
    payload?: Record<string, unknown>,
  ) => Effect.Effect<unknown, PiRpcError>;
  /**
   * Send a command and decode its `data` payload through `schema`.
   */
  readonly requestAs: <S extends Schema.Top>(
    command: string,
    schema: S,
    payload?: Record<string, unknown>,
  ) => Effect.Effect<S["Type"], PiRpcError, S["DecodingServices"]>;
  /** Answer a blocking `extension_ui_request`. */
  readonly respondToExtensionUi: (
    response: Record<string, unknown> & { readonly id: string },
  ) => Effect.Effect<void, PiRpcError>;
  /** Everything pi wrote on stdout, already routed and decoded. */
  readonly incoming: Stream.Stream<PiRpcIncoming>;
  /** Resolves once the process has exited; carries the terminal error. */
  readonly awaitExit: Effect.Effect<PiProcessExitedError>;
  /** Best-effort graceful shutdown: close stdin, then terminate. */
  readonly close: Effect.Effect<void>;
}

interface PendingCommand {
  readonly command: string;
  readonly deferred: Deferred.Deferred<unknown, PiRpcError>;
}

/**
 * Split a buffer into complete LF-delimited records. Returns the records and
 * the unterminated tail. Exported for unit testing: the framing rules (LF only,
 * optional trailing CR, no Unicode separators) are the part most likely to be
 * broken by a well-meaning refactor.
 */
export function splitJsonlBuffer(buffer: string): {
  readonly lines: ReadonlyArray<string>;
  readonly remainder: string;
} {
  const lines: Array<string> = [];
  let start = 0;
  for (;;) {
    const index = buffer.indexOf("\n", start);
    if (index === -1) break;
    const raw = buffer.slice(start, index);
    lines.push(raw.endsWith("\r") ? raw.slice(0, -1) : raw);
    start = index + 1;
  }
  return { lines, remainder: buffer.slice(start) };
}

function appendStderrTail(current: string, chunk: string): string {
  const next = current + chunk;
  return next.length > STDERR_TAIL_LIMIT ? next.slice(-STDERR_TAIL_LIMIT) : next;
}

export const makePiRpcConnection = Effect.fn("makePiRpcConnection")(function* (
  options: PiRpcConnectionOptions,
): Effect.fn.Return<
  PiRpcConnection,
  PiSpawnError,
  ChildProcessSpawner.ChildProcessSpawner | Scope.Scope
> {
  const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
  const connectionScope = yield* Scope.Scope;

  const incoming = yield* Queue.unbounded<PiRpcIncoming>();
  const outgoing = yield* Queue.unbounded<string, Cause.Done<void>>();
  const pending = yield* Ref.make(new Map<string, PendingCommand>());
  const nextRequestId = yield* Ref.make(1);
  const stdinRemainder = yield* Ref.make("");
  const stderrTail = yield* Ref.make("");
  const exitDeferred = yield* Deferred.make<PiProcessExitedError>();
  const terminated = yield* Ref.make(false);

  const log = (event: PiRpcLogEvent) => options.logger?.(event) ?? Effect.void;

  const spawnCommand = yield* resolveSpawnCommand(
    options.spawn.command,
    options.spawn.args,
    options.spawn.env ? { env: options.spawn.env, extendEnv: true } : {},
  ).pipe(Effect.mapError((cause) => new PiSpawnError({ command: options.spawn.command, cause })));

  const child = yield* spawner
    .spawn(
      ChildProcess.make(spawnCommand.command, spawnCommand.args, {
        cwd: options.spawn.cwd,
        ...(options.spawn.env ? { env: options.spawn.env, extendEnv: true } : {}),
        shell: spawnCommand.shell,
      }),
    )
    .pipe(
      Effect.provideService(Scope.Scope, connectionScope),
      Effect.mapError((cause) => new PiSpawnError({ command: options.spawn.command, cause })),
    );

  const failAllPending = (error: PiRpcError) =>
    Ref.getAndSet(pending, new Map<string, PendingCommand>()).pipe(
      Effect.flatMap((current) =>
        Effect.forEach([...current.values()], (entry) => Deferred.fail(entry.deferred, error), {
          discard: true,
        }),
      ),
    );

  /**
   * One-shot terminal transition. Both the stdout reader ending and the exit
   * watcher can get here first; `getAndSet` makes the loser a no-op so pending
   * commands are failed exactly once and the event queue closes once.
   */
  const handleTermination = Effect.fn("handleTermination")(function* (code: number | undefined) {
    if (yield* Ref.getAndSet(terminated, true)) {
      return;
    }
    const stderr = yield* Ref.get(stderrTail);
    const error = new PiProcessExitedError({
      ...(code === undefined ? {} : { code }),
      ...(stderr.trim().length > 0 ? { stderr } : {}),
    });
    yield* failAllPending(error);
    yield* Deferred.succeed(exitDeferred, error);
    yield* Queue.end(outgoing);
    yield* Queue.shutdown(incoming);
  });

  const settlePending = (response: PiRpcResponseEnvelope) =>
    Ref.modify(pending, (current) => {
      const key = response.id;
      if (key === undefined) {
        return [Effect.void, current] as const;
      }
      const entry = current.get(key);
      if (entry === undefined) {
        return [Effect.void, current] as const;
      }
      const next = new Map(current);
      next.delete(key);
      const settle = response.success
        ? Deferred.succeed(entry.deferred, response.data)
        : Deferred.fail(
            entry.deferred,
            new PiCommandError({
              command: entry.command,
              detail: response.error ?? "pi reported an unspecified failure.",
            }),
          );
      return [settle.pipe(Effect.asVoid), next] as const;
    }).pipe(Effect.flatten);

  const routeLine = Effect.fn("routeLine")(function* (line: string) {
    if (line.trim().length === 0) {
      return;
    }
    const parsed = yield* decodeJsonLine(line).pipe(Effect.option);
    if (parsed._tag === "None") {
      // A malformed line is a pi-side bug or interleaved output. Log and keep
      // reading: dropping the connection would lose an otherwise-healthy turn.
      yield* Effect.logWarning("Discarded unparseable pi RPC line.", {
        length: line.length,
      });
      return;
    }
    const value = parsed.value;
    yield* log({ direction: "incoming", payload: value });

    const type =
      typeof value === "object" && value !== null
        ? (value as { readonly type?: unknown }).type
        : undefined;

    if (type === "response") {
      const response = yield* decodeResponseEnvelope(value).pipe(Effect.option);
      if (response._tag === "None") {
        yield* Effect.logWarning("Discarded malformed pi RPC response line.");
        return;
      }
      yield* Queue.offer(incoming, { _tag: "Response", response: response.value });
      yield* settlePending(response.value);
      return;
    }

    if (type === "extension_ui_request") {
      const request = yield* decodeExtensionUiRequest(value).pipe(Effect.option);
      if (request._tag === "None") {
        yield* Effect.logWarning("Discarded malformed pi extension UI request.");
        return;
      }
      yield* Queue.offer(incoming, { _tag: "ExtensionUiRequest", request: request.value });
      return;
    }

    const event = yield* decodeAgentEvent(value).pipe(Effect.option);
    if (event._tag === "None") {
      yield* Effect.logDebug("Ignored pi RPC line without a usable event shape.");
      return;
    }
    yield* Queue.offer(incoming, { _tag: "Event", event: event.value, raw: value });
  });

  yield* child.stdout.pipe(
    Stream.decodeText(),
    Stream.runForEach((chunk) =>
      Ref.modify(stdinRemainder, (current) => {
        const { lines, remainder } = splitJsonlBuffer(current + chunk);
        return [lines, remainder] as const;
      }).pipe(Effect.flatMap((lines) => Effect.forEach(lines, routeLine, { discard: true }))),
    ),
    Effect.andThen(
      Ref.get(stdinRemainder).pipe(
        Effect.flatMap((tail) => (tail.trim().length > 0 ? routeLine(tail) : Effect.void)),
      ),
    ),
    Effect.andThen(
      child.exitCode.pipe(
        Effect.map((code) => Number(code)),
        Effect.orElseSucceed(() => undefined),
        Effect.flatMap(handleTermination),
      ),
    ),
    Effect.catchCause((cause) =>
      Effect.logWarning("pi RPC stdout reader failed.", { cause: Cause.pretty(cause) }).pipe(
        Effect.andThen(handleTermination(undefined)),
      ),
    ),
    Effect.forkIn(connectionScope),
  );

  yield* child.stderr.pipe(
    Stream.decodeText(),
    Stream.runForEach((chunk) => Ref.update(stderrTail, (tail) => appendStderrTail(tail, chunk))),
    Effect.ignore,
    Effect.forkIn(connectionScope),
  );

  yield* Stream.fromQueue(outgoing).pipe(
    Stream.encodeText,
    Stream.run(child.stdin),
    Effect.ignore,
    Effect.forkIn(connectionScope),
  );

  const send = (message: Record<string, unknown>) =>
    Effect.gen(function* () {
      if (yield* Ref.get(terminated)) {
        const stderr = yield* Ref.get(stderrTail);
        return yield* new PiProcessExitedError(stderr.trim().length > 0 ? { stderr } : {});
      }
      yield* log({ direction: "outgoing", payload: message });
      const encoded = yield* encodeJsonLine(message).pipe(
        Effect.mapError(
          (cause) =>
            new PiTransportError({
              operation: "encode-command",
              detail: `Failed to encode pi command '${String(message.type)}'.`,
              cause,
            }),
        ),
      );
      yield* Queue.offer(outgoing, `${encoded}\n`).pipe(Effect.asVoid);
    });

  const request: PiRpcConnection["request"] = (command, payload) =>
    Effect.gen(function* () {
      const requestId = yield* Ref.modify(
        nextRequestId,
        (current) => [`t3-${current}`, current + 1] as const,
      );
      const deferred = yield* Deferred.make<unknown, PiRpcError>();
      yield* Ref.update(pending, (current) =>
        new Map(current).set(requestId, { command, deferred }),
      );
      const removePending = Ref.update(pending, (current) => {
        if (!current.has(requestId)) return current;
        const next = new Map(current);
        next.delete(requestId);
        return next;
      });
      yield* send({ ...payload, id: requestId, type: command }).pipe(
        Effect.tapError(() => removePending),
      );
      return yield* Deferred.await(deferred).pipe(Effect.onInterrupt(() => removePending));
    });

  const requestAs: PiRpcConnection["requestAs"] = (command, schema, payload) =>
    request(command, payload).pipe(
      Effect.flatMap((data) =>
        Schema.decodeUnknownEffect(schema)(data).pipe(
          Effect.mapError(
            (cause) =>
              new PiDecodeError({
                command,
                detail: "Response payload did not match the expected shape.",
                cause,
              }),
          ),
        ),
      ),
    );

  const respondToExtensionUi: PiRpcConnection["respondToExtensionUi"] = (response) =>
    send({ ...response, type: "extension_ui_response" });

  const close = Effect.gen(function* () {
    yield* Queue.end(outgoing).pipe(Effect.ignore);
    yield* child.kill().pipe(Effect.ignore);
    yield* handleTermination(undefined).pipe(Effect.ignore);
  });

  yield* Effect.addFinalizer(() => close.pipe(Effect.ignore));

  return {
    pid: Number(child.pid),
    request,
    requestAs,
    respondToExtensionUi,
    incoming: Stream.fromQueue(incoming),
    awaitExit: Deferred.await(exitDeferred),
    close,
  } satisfies PiRpcConnection;
});
