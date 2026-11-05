/**
 * S5 stored two settings under names upstream now uses: server model mappings as
 * `usageModelMappings` (v0.2.43 to v0.2.63) and client gateway tags as
 * `usageModelAliases`. Files saved by those releases are read under the new names.
 */
import { ClientSettingsSchema } from "@t3tools/contracts";
import * as Schema from "effect/Schema";
import * as SchemaTransformation from "effect/SchemaTransformation";

function renameKey(document: unknown, from: string, to: string): unknown {
  if (typeof document !== "object" || document === null || Array.isArray(document)) {
    return document;
  }
  if (!Object.hasOwn(document, from) || Object.hasOwn(document, to)) return document;
  const { [from]: value, ...rest } = document as Record<string, unknown>;
  return { ...rest, [to]: value };
}

/** Reads `settings.json` text saved before the server key rename; other text passes through. */
export function adoptS5ServerSettingsKeys(raw: string): string {
  try {
    const document = JSON.parse(raw) as unknown;
    const renamed = renameKey(document, "usageModelMappings", "usageModelAliases");
    return renamed === document ? raw : JSON.stringify(renamed, null, 2);
  } catch {
    return raw;
  }
}

/** Client settings as persisted, accepting the pre-rename gateway tag key. */
export const PersistedClientSettings = Schema.Unknown.pipe(
  Schema.decodeTo(
    ClientSettingsSchema,
    SchemaTransformation.transform({
      // ClientSettingsSchema validates the result.
      decode: (document) =>
        renameKey(
          document,
          "usageModelAliases",
          "usageModelTags",
        ) as typeof ClientSettingsSchema.Encoded,
      encode: (settings) => settings,
    }),
  ),
);
