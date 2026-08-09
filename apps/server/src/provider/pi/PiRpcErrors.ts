/**
 * PiRpcErrors — transport-level failures for the `pi --mode rpc` connection.
 *
 * These sit below the adapter boundary. `PiAdapter` maps them onto
 * `ProviderAdapter*Error` so orchestration only ever sees provider-neutral
 * failures.
 *
 * @module provider/pi/PiRpcErrors
 */
import * as Schema from "effect/Schema";

/** Spawning the `pi` binary failed (missing binary, permissions, cwd). */
export class PiSpawnError extends Schema.TaggedErrorClass<PiSpawnError>()("PiSpawnError", {
  command: Schema.String,
  cause: Schema.optional(Schema.Defect()),
}) {
  override get message(): string {
    return `Failed to spawn pi CLI process '${this.command}'.`;
  }
}

/** stdin/stdout framing broke, or a line was not valid JSON. */
export class PiTransportError extends Schema.TaggedErrorClass<PiTransportError>()(
  "PiTransportError",
  {
    operation: Schema.String,
    detail: Schema.optional(Schema.String),
    cause: Schema.optional(Schema.Defect()),
  },
) {
  override get message(): string {
    return this.detail === undefined
      ? `pi RPC transport error during ${this.operation}.`
      : `pi RPC transport error during ${this.operation}: ${this.detail}`;
  }
}

/** The pi process exited. Terminal for the connection. */
export class PiProcessExitedError extends Schema.TaggedErrorClass<PiProcessExitedError>()(
  "PiProcessExitedError",
  {
    code: Schema.optional(Schema.Number),
    stderr: Schema.optional(Schema.String),
  },
) {
  override get message(): string {
    const code = this.code === undefined ? "unknown" : String(this.code);
    const stderr = this.stderr?.trim();
    return stderr
      ? `pi process exited with code ${code}: ${stderr}`
      : `pi process exited with code ${code}.`;
  }
}

/** pi answered a command with `success: false`. */
export class PiCommandError extends Schema.TaggedErrorClass<PiCommandError>()("PiCommandError", {
  command: Schema.String,
  detail: Schema.String,
}) {
  override get message(): string {
    return `pi command '${this.command}' failed: ${this.detail}`;
  }
}

/** A command response did not match the schema we expect for it. */
export class PiDecodeError extends Schema.TaggedErrorClass<PiDecodeError>()("PiDecodeError", {
  command: Schema.String,
  detail: Schema.String,
  cause: Schema.optional(Schema.Defect()),
}) {
  override get message(): string {
    return `pi command '${this.command}' returned unexpected data: ${this.detail}`;
  }
}

export type PiRpcError =
  | PiSpawnError
  | PiTransportError
  | PiProcessExitedError
  | PiCommandError
  | PiDecodeError;

const isPiRpcError = Schema.is(
  Schema.Union([
    PiSpawnError,
    PiTransportError,
    PiProcessExitedError,
    PiCommandError,
    PiDecodeError,
  ]),
);

/** Short human-readable detail for any cause, tagged or not. */
export function piRpcErrorDetail(cause: unknown): string {
  if (isPiRpcError(cause)) {
    return cause.message;
  }
  if (cause instanceof Error && cause.message.trim().length > 0) {
    return cause.message;
  }
  return "Unknown pi RPC failure.";
}
