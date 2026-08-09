/**
 * t3-runtime-mode — bundled pi extension that enforces S5 Code's runtime mode.
 *
 * pi has no permission protocol of its own: tools just run. The only channel a
 * pi process has for asking the user something is `ctx.ui.*`, which in RPC mode
 * becomes an `extension_ui_request` that blocks until the host answers. So the
 * only way to implement "ask before commands and file changes" is from inside
 * pi, via a `tool_call` hook that awaits `ctx.ui.confirm`.
 *
 * S5 Code loads this file with `--extension` and passes the mode in
 * `T3CODE_PI_RUNTIME_MODE`. In `full-access` the adapter does not load the
 * extension at all, so there is no hook on the tool path.
 *
 * Denials come back as `{ block: true }`, which pi reports to the model as a
 * failed tool call — the turn continues and the model can choose another route,
 * which is what a declined approval should do.
 *
 * The pi API types are declared locally rather than imported. This file is
 * loaded by pi's own loader at runtime, not bundled by us, and a structural
 * declaration of the two calls we make keeps the server package free of a
 * dependency on the pi CLI just to typecheck a 100-line bridge.
 *
 * @module pi-extension/t3-runtime-mode
 */

/** Mirrors `RuntimeMode` in `@t3tools/contracts`. */
type T3RuntimeMode = "approval-required" | "auto-accept-edits" | "auto" | "full-access";

/** Subset of pi's `ToolCallEvent` this hook reads. */
interface PiToolCallEvent {
  readonly toolName: string;
  readonly input: Record<string, unknown>;
}

/** Subset of pi's `ExtensionContext` this hook uses. */
interface PiToolCallContext {
  readonly ui: {
    readonly confirm: (title: string, message: string) => Promise<boolean>;
  };
}

/** Subset of pi's `ExtensionAPI` this extension registers against. */
interface PiExtensionApi {
  readonly on: (
    event: "tool_call",
    handler: (
      event: PiToolCallEvent,
      ctx: PiToolCallContext,
    ) => Promise<{ block?: boolean; reason?: string }>,
  ) => void;
}

const RUNTIME_MODE_ENV = "T3CODE_PI_RUNTIME_MODE";

/** pi built-ins that mutate the filesystem. */
const EDIT_TOOLS = new Set(["edit", "write"]);
/** pi built-ins that execute arbitrary commands. */
const EXEC_TOOLS = new Set(["bash"]);

export function parseRuntimeMode(value: string | undefined): T3RuntimeMode {
  switch (value) {
    case "approval-required":
    case "auto-accept-edits":
    case "auto":
      return value;
    default:
      return "full-access";
  }
}

/**
 * Whether a tool needs confirmation under the active mode.
 *
 * `auto` gates the same set as `auto-accept-edits`: T3's `auto` delegates
 * routine approvals to a reviewer model, which pi has no equivalent for, so the
 * honest behavior is to keep asking for command execution rather than to imply
 * a review happened.
 *
 * Read-only built-ins and extension tools are never gated. Gating every custom
 * tool would make the mode unusable, and the ones that actually mutate state do
 * it through bash/edit/write.
 */
export function requiresConfirmation(mode: T3RuntimeMode, toolName: string): boolean {
  if (mode === "full-access") return false;
  if (EXEC_TOOLS.has(toolName)) return true;
  if (EDIT_TOOLS.has(toolName)) return mode === "approval-required";
  return false;
}

/** One-line summary of what the tool is about to do, for the dialog body. */
export function describeToolCall(event: PiToolCallEvent): string {
  if (event.toolName === "bash") {
    const command = typeof event.input.command === "string" ? event.input.command : "";
    return command.trim().length > 0 ? command : "Run a shell command";
  }
  const path = typeof event.input.path === "string" ? event.input.path : undefined;
  if (path !== undefined && path.length > 0) {
    return `${event.toolName === "write" ? "Write" : "Edit"} ${path}`;
  }
  return `Run ${event.toolName}`;
}

export default function (pi: PiExtensionApi) {
  const mode = parseRuntimeMode(process.env[RUNTIME_MODE_ENV]);
  if (mode === "full-access") {
    return;
  }

  pi.on("tool_call", async (event, ctx) => {
    if (!requiresConfirmation(mode, event.toolName)) {
      return {};
    }

    // `ctx.ui.confirm` is boolean-only, so "accept for session" collapses to
    // "accept" here. The adapter records the session-wide decision instead, and
    // answers subsequent confirms without re-asking the user.
    const confirmed = await ctx.ui.confirm(`Allow ${event.toolName}?`, describeToolCall(event));
    return confirmed ? {} : { block: true, reason: "Denied by the user in S5 Code." };
  });
}
