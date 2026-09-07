/**
 * OpenCode2TextGeneration — structured text generation for OpenCode 2.
 *
 * @module textGeneration/OpenCode2TextGeneration
 */
import {
  TextGenerationError,
  type ChatAttachment,
  type ModelSelection,
  type OpenCode2Settings,
} from "@t3tools/contracts";
import { sanitizeBranchFragment, sanitizeFeatureBranchName } from "@t3tools/shared/git";
import { getModelSelectionStringOptionValue } from "@t3tools/shared/model";
import { extractJsonObject } from "@t3tools/shared/schemaJson";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";

import { OpenCode2HostHandle } from "../provider/OpenCode2Host.ts";
import * as TextGeneration from "./TextGeneration.ts";
import {
  buildBranchNamePrompt,
  buildCommitMessagePrompt,
  buildPrContentPrompt,
  buildThreadTitlePrompt,
} from "./TextGenerationPrompts.ts";
import {
  sanitizeCommitSubject,
  sanitizePrTitle,
  sanitizeThreadTitle,
} from "./TextGenerationUtils.ts";

function parseModelSlug(slug: string): { providerID: string; modelID: string } | null {
  const slash = slug.indexOf("/");
  if (slash <= 0 || slash === slug.length - 1) {
    return null;
  }
  return {
    providerID: slug.slice(0, slash),
    modelID: slug.slice(slash + 1),
  };
}

export const makeOpenCode2TextGeneration = (
  hostHandle: OpenCode2HostHandle,
  _settings: OpenCode2Settings,
): TextGeneration.TextGeneration["Service"] => {
  const runOpenCode2Json = Effect.fn("runOpenCode2Json")(function* <S extends Schema.Top>(input: {
    readonly operation: string;
    readonly cwd: string;
    readonly prompt: string;
    readonly outputSchemaJson: S;
    readonly modelSelection: ModelSelection;
    readonly attachments?: ReadonlyArray<ChatAttachment> | undefined;
  }) {
    const parsedModel = parseModelSlug(input.modelSelection.model);
    const selectedVariant = getModelSelectionStringOptionValue(input.modelSelection, "variant");

    // Attempt direct generate.text first
    const generatedTextResult = yield* Effect.tryPromise({
      try: async () => {
        if (typeof hostHandle.client.generate?.text === "function") {
          const res = await hostHandle.client.generate.text({
            prompt: input.prompt,
            ...(parsedModel
              ? {
                  model: {
                    id: parsedModel.modelID,
                    providerID: parsedModel.providerID,
                    ...(selectedVariant ? { variant: selectedVariant } : {}),
                  },
                }
              : {}),
          });
          const text =
            (res as { text?: string; output?: string })?.text ??
            (res as { output?: string })?.output;
          if (typeof text === "string" && text.trim().length > 0) {
            return text.trim();
          }
        }
        // Fallback: create an ephemeral session, prompt it, and remove it
        const session = await hostHandle.client.session.create({
          directory: input.cwd,
          title: `T3 Text Gen: ${input.operation}`,
        });
        try {
          const promptRes = await hostHandle.client.session.prompt({
            sessionID: session.id,
            text: input.prompt,
            delivery: "steer",
          });
          // Retrieve response text from prompt result or session log
          const parts =
            (promptRes as { parts?: Array<{ type?: string; text?: string }> })?.parts ?? [];
          const textPart = parts.find((p) => p.type === "text" && typeof p.text === "string");
          if (textPart?.text) {
            return textPart.text;
          }
          return "";
        } finally {
          await hostHandle.client.session.remove({ sessionID: session.id }).catch(() => {});
        }
      },
      catch: (cause) =>
        new TextGenerationError({
          operation: input.operation,
          detail: `OpenCode 2 text generation failed: ${String(cause)}`,
          cause,
        }),
    });

    const decodeOutput = Schema.decodeEffect(Schema.fromJsonString(input.outputSchemaJson));
    return yield* decodeOutput(extractJsonObject(generatedTextResult)).pipe(
      Effect.catchTags({
        SchemaError: (cause) =>
          Effect.fail(
            new TextGenerationError({
              operation: input.operation,
              detail: "OpenCode 2 returned invalid structured output.",
              cause,
            }),
          ),
      }),
    );
  });

  const generateCommitMessage: TextGeneration.TextGeneration["Service"]["generateCommitMessage"] =
    Effect.fn("OpenCode2TextGeneration.generateCommitMessage")(function* (input) {
      const { prompt, outputSchema } = buildCommitMessagePrompt({
        branch: input.branch,
        stagedSummary: input.stagedSummary,
        stagedPatch: input.stagedPatch,
        includeBranch: input.includeBranch === true,
        policy: input.policy,
      });
      const generated = yield* runOpenCode2Json({
        operation: "generateCommitMessage",
        cwd: input.cwd,
        prompt,
        outputSchemaJson: outputSchema,
        modelSelection: input.modelSelection,
      });

      return {
        subject: sanitizeCommitSubject(generated.subject),
        body: generated.body.trim(),
        ...("branch" in generated && typeof generated.branch === "string"
          ? { branch: sanitizeFeatureBranchName(generated.branch) }
          : {}),
      };
    });

  const generatePrContent: TextGeneration.TextGeneration["Service"]["generatePrContent"] =
    Effect.fn("OpenCode2TextGeneration.generatePrContent")(function* (input) {
      const { prompt, outputSchema } = buildPrContentPrompt({
        baseBranch: input.baseBranch,
        headBranch: input.headBranch,
        commitSummary: input.commitSummary,
        diffSummary: input.diffSummary,
        diffPatch: input.diffPatch,
        policy: input.policy,
        changeRequestTemplate: input.changeRequestTemplate,
      });
      const generated = yield* runOpenCode2Json({
        operation: "generatePrContent",
        cwd: input.cwd,
        prompt,
        outputSchemaJson: outputSchema,
        modelSelection: input.modelSelection,
      });

      return {
        title: sanitizePrTitle(generated.title),
        body: generated.body.trim(),
      };
    });

  const generateBranchName: TextGeneration.TextGeneration["Service"]["generateBranchName"] =
    Effect.fn("OpenCode2TextGeneration.generateBranchName")(function* (input) {
      const { prompt, outputSchema } = buildBranchNamePrompt({
        message: input.message,
        attachments: input.attachments,
      });
      const generated = yield* runOpenCode2Json({
        operation: "generateBranchName",
        cwd: input.cwd,
        prompt,
        outputSchemaJson: outputSchema,
        modelSelection: input.modelSelection,
        attachments: input.attachments,
      });

      return {
        branch: sanitizeBranchFragment(generated.branch),
      };
    });

  const generateThreadTitle: TextGeneration.TextGeneration["Service"]["generateThreadTitle"] =
    Effect.fn("OpenCode2TextGeneration.generateThreadTitle")(function* (input) {
      const { prompt, outputSchema } = buildThreadTitlePrompt({
        message: input.message,
        previousTitle: input.previousTitle,
        attachments: input.attachments,
      });
      const generated = yield* runOpenCode2Json({
        operation: "generateThreadTitle",
        cwd: input.cwd,
        prompt,
        outputSchemaJson: outputSchema,
        modelSelection: input.modelSelection,
        attachments: input.attachments,
      });

      return {
        title: sanitizeThreadTitle(generated.title),
      };
    });

  return {
    generateCommitMessage,
    generatePrContent,
    generateBranchName,
    generateThreadTitle,
  };
};
