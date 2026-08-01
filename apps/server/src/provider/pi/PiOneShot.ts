/**
 * PiOneShot — run a single non-interactive prompt through `pi --mode rpc`.
 *
 * Used by text generation (commit messages, PR content, branch names, thread
 * titles). pi has no structured-output flag like Claude's `--json-schema`, so
 * the JSON contract lives in the prompt and the answer is parsed out of the
 * assistant's final text.
 *
 * Two flags matter for correctness here:
 *   - `--no-session` keeps these throwaway runs out of the user's pi history.
 *   - `--no-tools` removes every tool, including extension tools. A text
 *     generation prompt has no reason to touch the filesystem, and leaving
 *     tools enabled lets the model burn turns on them before answering.
 *
 * `prompt` returns as soon as pi accepts the work, so completion is detected
 * from the `agent_end` event rather than the command response.
 *
 * @module provider/pi/PiOneShot
 */
import type { PiSettings } from "@t3tools/contracts";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import { ChildProcessSpawner } from "effect/unstable/process";

import { resolvePiLaunch } from "./PiLaunch.ts";
import { parsePiModelSlug, type PiModelRef } from "./PiModelSupport.ts";
import { makePiRpcConnection } from "./PiRpcConnection.ts";
import { PiCommandError, type PiRpcError } from "./PiRpcErrors.ts";
import { piAssistantText, piContentBlocks } from "./PiTurnState.ts";
import type { PiThinkingLevel } from "./PiRpcSchemas.ts";

export interface PiOneShotInput {
  readonly piSettings: PiSettings;
  readonly environment: NodeJS.ProcessEnv;
  readonly cwd: string;
  readonly prompt: string;
  readonly model: string;
  readonly thinkingLevel?: PiThinkingLevel | undefined;
}

const LastAssistantText = Schema.Struct({
  text: Schema.optional(Schema.NullOr(Schema.String)),
});

/**
 * Run one prompt and return the assistant's final text.
 *
 * Text is taken from the `agent_end` payload when present and falls back to
 * `get_last_assistant_text`; the fallback covers runs where the final message
 * was persisted but not echoed on the event.
 */
export const runPiOneShotPrompt = (
  input: PiOneShotInput,
): Effect.Effect<string, PiRpcError, ChildProcessSpawner.ChildProcessSpawner> =>
  Effect.gen(function* () {
    const parsedModel: PiModelRef | undefined = parsePiModelSlug(input.model);
    if (parsedModel === undefined) {
      return yield* new PiCommandError({
        command: "set_model",
        detail: `pi model '${input.model}' must use the 'provider/model' format.`,
      });
    }

    const launch = resolvePiLaunch({
      piSettings: input.piSettings,
      environment: input.environment,
      extraArgs: ["--no-session", "--no-tools"],
    });
    const connection = yield* makePiRpcConnection({
      spawn: {
        command: launch.command,
        args: launch.args,
        cwd: input.cwd,
        env: launch.env,
      },
    });

    const finished = yield* Deferred.make<string | undefined>();
    yield* connection.incoming.pipe(
      Stream.runForEach((incoming) =>
        incoming._tag === "Event" && incoming.event.type === "agent_end"
          ? Deferred.succeed(finished, lastAssistantTextFromAgentEnd(incoming.event.messages)).pipe(
              Effect.asVoid,
            )
          : Effect.void,
      ),
      Effect.ignore,
      Effect.forkScoped,
    );
    // A pi exit before `agent_end` would otherwise hang this fiber forever.
    yield* connection.awaitExit.pipe(
      Effect.flatMap(() => Deferred.succeed(finished, undefined)),
      Effect.ignore,
      Effect.forkScoped,
    );

    yield* connection.request("set_model", {
      provider: parsedModel.provider,
      modelId: parsedModel.modelId,
    });
    if (input.thinkingLevel !== undefined) {
      yield* connection.request("set_thinking_level", { level: input.thinkingLevel });
    }
    yield* connection.request("prompt", { message: input.prompt });

    const streamedText = yield* Deferred.await(finished);
    if (streamedText !== undefined && streamedText.trim().length > 0) {
      return streamedText;
    }

    const fallback = yield* connection
      .requestAs("get_last_assistant_text", LastAssistantText)
      .pipe(Effect.orElseSucceed(() => ({ text: null })));
    return fallback.text ?? "";
  }).pipe(Effect.scoped);

function lastAssistantTextFromAgentEnd(messages: unknown): string | undefined {
  if (!Array.isArray(messages)) {
    return undefined;
  }
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const entry = messages[index];
    if (entry === null || typeof entry !== "object") continue;
    const record = entry as { readonly role?: unknown; readonly content?: unknown };
    if (record.role !== "assistant") continue;
    const text = piAssistantText(piContentBlocks(record.content));
    if (text.trim().length > 0) {
      return text;
    }
  }
  return undefined;
}
