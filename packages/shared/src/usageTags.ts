/**
 * Editing the user's model tags.
 *
 * A tag says "this model name is really that catalog entry". The awkward part
 * is identity: an untagged row stands for one transcript name, but a tagged row
 * is a merge of every name pointed at one catalog entry, so editing it has to
 * move all of them together or the row would split in half.
 *
 * @module usageTags
 */
import type { UsageCatalogModelId, UsageModelAlias, UsageProviderKind } from "@t3tools/contracts";

/** The row the tag dialog is editing. */
export interface UsageModelTagTarget {
  readonly provider: UsageProviderKind;
  /** What to call the row in the dialog. */
  readonly label: string;
  /**
   * The transcript identity to tag, for a row that is not tagged yet.
   *
   * Null once a row is tagged: consolidation merges every gateway name the user
   * pointed at one catalog entry, so the row no longer stands for a single
   * transcript name and is addressed by its catalog entry instead.
   */
  readonly untagged: { readonly apiProvider: string; readonly model: string } | null;
  /** The catalog entry this row is already tagged as, when it is. */
  readonly taggedAs: string | null;
}

/**
 * Whether an existing tag is one of the tags a dialog target covers.
 *
 * An untagged target owns exactly one transcript name. A tagged target owns
 * every name the user pointed at its catalog entry, because that is what got
 * merged into the single row they are looking at.
 */
export function matchesTarget(alias: UsageModelAlias, target: UsageModelTagTarget): boolean {
  if (alias.provider !== target.provider) return false;
  return target.untagged === null
    ? alias.catalogModelId === target.taggedAs
    : alias.apiProvider === target.untagged.apiProvider && alias.model === target.untagged.model;
}

/** Points the target at a catalog entry, replacing whatever it said before. */
export function applyTag(
  aliases: readonly UsageModelAlias[],
  target: UsageModelTagTarget,
  catalogModelId: UsageCatalogModelId,
): readonly UsageModelAlias[] {
  // Retagging a consolidated row moves every name that fed it, so the row stays
  // whole instead of splitting between the old and new answers.
  const remapped = aliases.map((alias) =>
    matchesTarget(alias, target) ? { ...alias, catalogModelId } : alias,
  );
  if (target.untagged === null) return remapped;

  return [
    ...remapped.filter((alias) => !matchesTarget(alias, target)),
    {
      provider: target.provider,
      apiProvider: target.untagged.apiProvider,
      model: target.untagged.model,
      catalogModelId,
    },
  ];
}

/** Drops the target's tags, returning its usage to unpriced. */
export function clearTag(
  aliases: readonly UsageModelAlias[],
  target: UsageModelTagTarget,
): readonly UsageModelAlias[] {
  return aliases.filter((alias) => !matchesTarget(alias, target));
}
