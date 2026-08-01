/**
 * PiRpcSchemas — wire schemas for the subset of `pi --mode rpc` we consume.
 *
 * pi's RPC surface is large and evolves; decoding it wholesale would make the
 * adapter fail on a pi upgrade that adds a field. Every schema here is
 * deliberately permissive: unknown keys pass through, payloads we only forward
 * (models, messages, tool results) stay `Schema.Unknown`, and the event union
 * ends in a catch-all so an unrecognised `type` is data, not an error.
 *
 * @module provider/pi/PiRpcSchemas
 */
import * as Schema from "effect/Schema";

/** Thinking levels pi accepts on `set_thinking_level`. */
export const PiThinkingLevel = Schema.Literals([
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
]);
export type PiThinkingLevel = typeof PiThinkingLevel.Type;

const isPiThinkingLevel = Schema.is(PiThinkingLevel);

export function parsePiThinkingLevel(value: string | undefined): PiThinkingLevel | undefined {
  if (value === undefined) return undefined;
  const normalized = value.trim().toLowerCase();
  return isPiThinkingLevel(normalized) ? normalized : undefined;
}

/**
 * One entry from `get_available_models`. `thinkingLevelMap` is pi's per-model
 * mapping from our canonical level names to provider-specific values; its keys
 * are the authoritative set of levels that model accepts.
 */
export const PiModel = Schema.Struct({
  id: Schema.String,
  name: Schema.optional(Schema.String),
  provider: Schema.String,
  api: Schema.optional(Schema.String),
  reasoning: Schema.optional(Schema.Boolean),
  input: Schema.optional(Schema.Array(Schema.String)),
  contextWindow: Schema.optional(Schema.Number),
  maxTokens: Schema.optional(Schema.Number),
  thinkingLevelMap: Schema.optional(Schema.Record(Schema.String, Schema.Unknown)),
});
export type PiModel = typeof PiModel.Type;

export const PiAvailableModels = Schema.Struct({
  models: Schema.Array(PiModel),
});

export const PiSessionState = Schema.Struct({
  model: Schema.optional(Schema.NullOr(PiModel)),
  thinkingLevel: Schema.optional(Schema.String),
  isStreaming: Schema.optional(Schema.Boolean),
  isCompacting: Schema.optional(Schema.Boolean),
  sessionFile: Schema.optional(Schema.String),
  sessionId: Schema.String,
  sessionName: Schema.optional(Schema.String),
  autoCompactionEnabled: Schema.optional(Schema.Boolean),
  messageCount: Schema.optional(Schema.Number),
  pendingMessageCount: Schema.optional(Schema.Number),
});
export type PiSessionState = typeof PiSessionState.Type;

export const PiCommand = Schema.Struct({
  name: Schema.String,
  description: Schema.optional(Schema.String),
  source: Schema.optional(Schema.String),
});
export type PiCommand = typeof PiCommand.Type;

export const PiCommands = Schema.Struct({
  commands: Schema.Array(PiCommand),
});

/**
 * Session entry from `get_entries`. Only the tree shape matters to us: ids and
 * parent links form the durable cursor space pi's `fork` command addresses.
 */
export const PiSessionEntry = Schema.Struct({
  id: Schema.String,
  type: Schema.optional(Schema.String),
  parentId: Schema.optional(Schema.NullOr(Schema.String)),
  timestamp: Schema.optional(Schema.String),
  message: Schema.optional(Schema.Unknown),
});
export type PiSessionEntry = typeof PiSessionEntry.Type;

export const PiEntries = Schema.Struct({
  entries: Schema.Array(PiSessionEntry),
  leafId: Schema.optional(Schema.NullOr(Schema.String)),
});

export const PiTokenUsage = Schema.Struct({
  input: Schema.optional(Schema.Number),
  output: Schema.optional(Schema.Number),
  cacheRead: Schema.optional(Schema.Number),
  cacheWrite: Schema.optional(Schema.Number),
  totalTokens: Schema.optional(Schema.Number),
  cost: Schema.optional(Schema.Unknown),
});
export type PiTokenUsage = typeof PiTokenUsage.Type;

/** Envelope shared by every `type: "response"` line. */
export const PiRpcResponseEnvelope = Schema.Struct({
  type: Schema.Literal("response"),
  id: Schema.optional(Schema.String),
  command: Schema.String,
  success: Schema.Boolean,
  data: Schema.optional(Schema.Unknown),
  error: Schema.optional(Schema.String),
});
export type PiRpcResponseEnvelope = typeof PiRpcResponseEnvelope.Type;

/**
 * Extension UI request. pi routes `ctx.ui.*` calls from extensions through
 * this; the dialog methods (`select`/`confirm`/`input`/`editor`) block the
 * extension until we send back a matching `extension_ui_response`.
 */
export const PiExtensionUiRequest = Schema.Struct({
  type: Schema.Literal("extension_ui_request"),
  id: Schema.String,
  method: Schema.String,
  title: Schema.optional(Schema.String),
  message: Schema.optional(Schema.String),
  options: Schema.optional(Schema.Array(Schema.String)),
  placeholder: Schema.optional(Schema.String),
  prefill: Schema.optional(Schema.String),
  timeout: Schema.optional(Schema.Number),
  notifyType: Schema.optional(Schema.String),
  statusKey: Schema.optional(Schema.String),
  statusText: Schema.optional(Schema.NullOr(Schema.String)),
  widgetKey: Schema.optional(Schema.String),
  widgetLines: Schema.optional(Schema.NullOr(Schema.Array(Schema.String))),
  text: Schema.optional(Schema.String),
});
export type PiExtensionUiRequest = typeof PiExtensionUiRequest.Type;

/** Dialog methods block the extension and therefore need a response. */
export const PI_EXTENSION_UI_DIALOG_METHODS = ["select", "confirm", "input", "editor"] as const;
export type PiExtensionUiDialogMethod = (typeof PI_EXTENSION_UI_DIALOG_METHODS)[number];

export function isPiExtensionUiDialogMethod(method: string): method is PiExtensionUiDialogMethod {
  return (PI_EXTENSION_UI_DIALOG_METHODS as ReadonlyArray<string>).includes(method);
}

/**
 * Streaming delta carried by `message_update`. Only the discriminant and the
 * text-bearing fields are typed; the accumulating `partial` message is left
 * out because we rebuild assistant text from deltas.
 */
export const PiAssistantMessageEvent = Schema.Struct({
  type: Schema.String,
  contentIndex: Schema.optional(Schema.Number),
  delta: Schema.optional(Schema.String),
  content: Schema.optional(Schema.Unknown),
  toolCall: Schema.optional(Schema.Unknown),
  reason: Schema.optional(Schema.String),
});
export type PiAssistantMessageEvent = typeof PiAssistantMessageEvent.Type;

const PiEventBase = { type: Schema.String } as const;

/**
 * Agent event stream. A permissive struct rather than a tagged union: pi adds
 * event types between releases and an unknown `type` must be inert, not fatal.
 */
export const PiAgentEvent = Schema.Struct({
  ...PiEventBase,
  message: Schema.optional(Schema.Unknown),
  messages: Schema.optional(Schema.Unknown),
  assistantMessageEvent: Schema.optional(PiAssistantMessageEvent),
  toolCallId: Schema.optional(Schema.String),
  toolName: Schema.optional(Schema.String),
  args: Schema.optional(Schema.Unknown),
  result: Schema.optional(Schema.Unknown),
  partialResult: Schema.optional(Schema.Unknown),
  isError: Schema.optional(Schema.Boolean),
  toolResults: Schema.optional(Schema.Unknown),
  willRetry: Schema.optional(Schema.Boolean),
  aborted: Schema.optional(Schema.Boolean),
  reason: Schema.optional(Schema.String),
  errorMessage: Schema.optional(Schema.String),
  attempt: Schema.optional(Schema.Number),
  maxAttempts: Schema.optional(Schema.Number),
  delayMs: Schema.optional(Schema.Number),
  finalError: Schema.optional(Schema.String),
  success: Schema.optional(Schema.Boolean),
  steering: Schema.optional(Schema.Array(Schema.String)),
  followUp: Schema.optional(Schema.Array(Schema.String)),
  delta: Schema.optional(Schema.String),
  id: Schema.optional(Schema.String),
  extensionPath: Schema.optional(Schema.String),
  event: Schema.optional(Schema.String),
  error: Schema.optional(Schema.String),
});
export type PiAgentEvent = typeof PiAgentEvent.Type;

/** Every line pi writes to stdout, after routing. */
export type PiRpcIncoming =
  | { readonly _tag: "Response"; readonly response: PiRpcResponseEnvelope }
  | { readonly _tag: "ExtensionUiRequest"; readonly request: PiExtensionUiRequest }
  | { readonly _tag: "Event"; readonly event: PiAgentEvent; readonly raw: unknown };

/**
 * Assistant message content block. pi emits `text`, `thinking`, and `toolCall`
 * blocks; we read text and thinking to reconstruct final message bodies when a
 * stream was interrupted before `message_end`.
 */
export const PiAssistantContentBlock = Schema.Struct({
  type: Schema.String,
  text: Schema.optional(Schema.String),
  thinking: Schema.optional(Schema.String),
  id: Schema.optional(Schema.String),
  name: Schema.optional(Schema.String),
  arguments: Schema.optional(Schema.Unknown),
});
export type PiAssistantContentBlock = typeof PiAssistantContentBlock.Type;
