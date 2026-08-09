/**
 * Maps a model name we could not price onto a real catalog entry.
 *
 * Gateways report names no catalog knows (`cline-free/glm-5.2`) or names
 * several catalogs price differently (`claude-opus-5` is served by eleven
 * providers at three prices). Rather than guess, those rows report as unpriced
 * and this is how a user says what they actually are. The tag is stored in
 * client settings, so tagging once covers every environment.
 *
 * @module UsageModelTagDialog
 */
import { useDeferredValue, useState } from "react";

import { useUsageModelSearch } from "../../state/usage";
import type { UsageModelTagTarget } from "../../usage/usageTags";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogPanel,
  DialogPopup,
  DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { ScrollArea } from "../ui/scroll-area";

interface UsageModelTagDialogProps {
  readonly target: UsageModelTagTarget | null;
  readonly onClose: () => void;
  readonly onTag: (catalogModelId: string) => void;
  readonly onClear: () => void;
}

const COST = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  maximumFractionDigits: 2,
});

export function UsageModelTagDialog({ target, onClose, onTag, onClear }: UsageModelTagDialogProps) {
  const [query, setQuery] = useState("");
  // The search is a round trip per keystroke otherwise. Deferring keeps typing
  // responsive and collapses bursts into one query.
  const deferredQuery = useDeferredValue(query);
  const { models, isPending } = useUsageModelSearch(deferredQuery);

  return (
    <Dialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open) {
          setQuery("");
          onClose();
        }
      }}
    >
      <DialogPopup className="max-w-2xl overflow-hidden">
        <DialogHeader>
          <DialogTitle>Tag {target?.label}</DialogTitle>
          <DialogDescription>
            {target?.taggedAs !== null && target !== null
              ? `Currently counted as ${target.taggedAs}. Pick a different model, or remove the tag to leave this usage unpriced.`
              : target?.untagged?.apiProvider
                ? `Served through ${target.untagged.apiProvider}. Pick the model it really is, and its rates apply to this usage everywhere.`
                : "Pick the model this really is, and its rates apply to this usage everywhere."}
          </DialogDescription>
        </DialogHeader>
        <DialogPanel className="space-y-3">
          <Input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search models…"
          />
          <ScrollArea className="h-72">
            <div className="flex flex-col gap-1 pr-3">
              {deferredQuery.trim().length === 0 ? (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  Search the models.dev catalog by model or provider name.
                </p>
              ) : models.length === 0 ? (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  {isPending ? "Searching…" : "No models match that search."}
                </p>
              ) : (
                models.map((model) => (
                  <button
                    key={model.id}
                    type="button"
                    onClick={() => {
                      setQuery("");
                      onTag(model.id);
                    }}
                    className={
                      "flex items-center justify-between gap-4 rounded-md px-3 py-2 text-left text-sm hover:bg-accent " +
                      (model.id === target?.taggedAs ? "bg-accent/50" : "")
                    }
                  >
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate text-foreground">{model.modelName}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        {model.providerName} · {model.id}
                      </span>
                    </span>
                    <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                      {COST.format(model.inputCostPerMillion)} in /{" "}
                      {COST.format(model.outputCostPerMillion)} out per 1M
                    </span>
                  </button>
                ))
              )}
            </div>
          </ScrollArea>
        </DialogPanel>
        <DialogFooter>
          {/* The way out: a tag a user regrets has to be removable. */}
          {target?.taggedAs === null || target === null ? null : (
            <Button
              variant="ghost"
              onClick={() => {
                setQuery("");
                onClear();
              }}
            >
              Remove tag
            </Button>
          )}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
        </DialogFooter>
      </DialogPopup>
    </Dialog>
  );
}
