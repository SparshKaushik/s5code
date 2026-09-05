import { useCallback, useMemo, useState } from "react";
import { HardDriveIcon, RefreshCwIcon } from "lucide-react";
import {
  isAtomCommandInterrupted,
  squashAtomCommandFailure,
} from "@t3tools/client-runtime/state/runtime";
import {
  DEFAULT_CHECKPOINT_RETENTION_DAYS,
  DEFAULT_CHECKPOINT_RETENTION_MEGABYTES,
  MAX_CHECKPOINT_RETENTION_DAYS,
  MAX_CHECKPOINT_RETENTION_MEGABYTES,
  MIN_CHECKPOINT_RETENTION_DAYS,
  MIN_CHECKPOINT_RETENTION_MEGABYTES,
  type CheckpointCleanupScope,
} from "@t3tools/contracts";

import { usePrimarySettings, useUpdatePrimarySettings } from "../../hooks/useSettings";
import { usePrimaryEnvironment } from "../../state/environments";
import { useEnvironmentQuery } from "../../state/query";
import { checkpointMaintenanceEnvironment } from "../../state/checkpointMaintenance";
import { useAtomCommand } from "../../state/use-atom-command";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogPopup,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import { Button } from "../ui/button";
import {
  NumberField,
  NumberFieldDecrement,
  NumberFieldGroup,
  NumberFieldIncrement,
  NumberFieldInput,
} from "../ui/number-field";
import { Switch } from "../ui/switch";
import { toastManager } from "../ui/toast";
import {
  describeCleanupScope,
  formatCleanupResultDescription,
  summarizeCheckpointUsage,
} from "./ExperimentalSettings.logic";
import { SettingsPageContainer, SettingsRow, SettingsSection } from "./settingsLayout";

const CLEANUP_SCOPES: ReadonlyArray<CheckpointCleanupScope> = [
  "orphaned",
  "retention-policy",
  "all",
];

function CheckpointStorageSection() {
  const environmentId = usePrimaryEnvironment()?.environmentId ?? null;
  const [pendingScope, setPendingScope] = useState<CheckpointCleanupScope | null>(null);
  const [runningScope, setRunningScope] = useState<CheckpointCleanupScope | null>(null);
  const runCleanup = useAtomCommand(checkpointMaintenanceEnvironment.cleanup, {
    reportFailure: false,
  });
  const { data, error, isPending, refresh } = useEnvironmentQuery(
    environmentId === null
      ? null
      : checkpointMaintenanceEnvironment.usage({ environmentId, input: {} }),
  );
  const summary = useMemo(() => (data ? summarizeCheckpointUsage(data) : null), [data]);

  const confirmCleanup = useCallback(
    (scope: CheckpointCleanupScope) => {
      if (environmentId === null) return;
      setPendingScope(null);
      setRunningScope(scope);
      void (async () => {
        const result = await runCleanup({ environmentId, input: { scope } });
        setRunningScope(null);
        if (result._tag === "Failure") {
          if (!isAtomCommandInterrupted(result)) {
            const failure = squashAtomCommandFailure(result);
            toastManager.add({
              type: "error",
              title: "Checkpoint cleanup failed",
              description:
                failure instanceof Error ? failure.message : "The cleanup could not complete.",
            });
          }
          return;
        }
        toastManager.add({
          type: "success",
          title: "Checkpoint cleanup complete",
          description: formatCleanupResultDescription({
            removedEntryCount: result.value.removedEntries.length,
            removedRefCount: result.value.removedRefCount,
            reclaimedBytes: result.value.reclaimedBytes,
          }),
        });
        refresh();
      })();
    },
    [environmentId, refresh, runCleanup],
  );

  const pendingCopy = pendingScope === null ? null : describeCleanupScope(pendingScope);

  return (
    <SettingsSection
      title="Checkpoint storage"
      icon={<HardDriveIcon className="size-4 text-muted-foreground" />}
      headerAction={
        <Button
          size="icon-xs"
          variant="ghost"
          aria-label="Refresh checkpoint storage usage"
          disabled={environmentId === null || isPending}
          onClick={refresh}
        >
          <RefreshCwIcon className="size-3.5" />
        </Button>
      }
    >
      <SettingsRow
        title="Space used"
        description="Hidden checkpoint commits inside your repositories. Your branches, tags, and commits are never counted or touched."
        status={
          error !== null ? (
            <span className="text-destructive">{error}</span>
          ) : summary === null ? (
            isPending ? (
              "Scanning repositories..."
            ) : (
              "Usage is unavailable."
            )
          ) : (
            `${summary.totalBytesLabel} across ${summary.entryCount} ${
              summary.entryCount === 1 ? "entry" : "entries"
            } (${summary.refCount} ${summary.refCount === 1 ? "checkpoint" : "checkpoints"}). ${
              summary.orphanedBytesLabel
            } belongs to ${summary.orphanedCount} deleted ${
              summary.orphanedCount === 1 ? "thread" : "threads"
            }.`
          )
        }
      />
      {CLEANUP_SCOPES.map((scope) => {
        const copy = describeCleanupScope(scope);
        return (
          <SettingsRow
            key={scope}
            title={copy.label}
            description={copy.confirmDescription}
            control={
              <Button
                variant={copy.destructive ? "destructive" : "outline"}
                size="sm"
                disabled={environmentId === null || runningScope !== null}
                onClick={() => setPendingScope(scope)}
              >
                {runningScope === scope ? "Cleaning up..." : copy.label}
              </Button>
            }
          />
        );
      })}
      <AlertDialog
        open={pendingScope !== null}
        onOpenChange={(open) => {
          if (!open) setPendingScope(null);
        }}
      >
        <AlertDialogPopup>
          <AlertDialogHeader>
            <AlertDialogTitle>{pendingCopy?.confirmTitle ?? ""}</AlertDialogTitle>
            <AlertDialogDescription>{pendingCopy?.confirmDescription ?? ""}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogClose render={<Button variant="outline" />}>Cancel</AlertDialogClose>
            <Button
              variant={pendingCopy?.destructive ? "destructive" : "default"}
              onClick={() => {
                if (pendingScope !== null) confirmCleanup(pendingScope);
              }}
            >
              {pendingCopy?.label ?? "Clean up"}
            </Button>
          </AlertDialogFooter>
        </AlertDialogPopup>
      </AlertDialog>
    </SettingsSection>
  );
}

function RetentionLimitControl({
  value,
  min,
  max,
  step,
  unitLabel,
  ariaLabel,
  onChange,
}: {
  value: number | null;
  min: number;
  max: number;
  step: number;
  unitLabel: string;
  ariaLabel: string;
  onChange: (next: number | null) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      {value === null ? null : (
        <>
          <NumberField
            value={value}
            min={min}
            max={max}
            step={step}
            size="sm"
            className="w-32"
            onValueChange={(next) => {
              if (next === null || !Number.isFinite(next)) return;
              onChange(Math.min(max, Math.max(min, Math.round(next))));
            }}
          >
            <NumberFieldGroup>
              <NumberFieldDecrement aria-label={`Decrease ${ariaLabel}`} />
              <NumberFieldInput aria-label={ariaLabel} />
              <NumberFieldIncrement aria-label={`Increase ${ariaLabel}`} />
            </NumberFieldGroup>
          </NumberField>
          <span className="text-xs text-muted-foreground">{unitLabel}</span>
        </>
      )}
      <Switch
        checked={value !== null}
        onCheckedChange={(checked) => onChange(checked ? min : null)}
        aria-label={`Enable ${ariaLabel}`}
      />
    </div>
  );
}

function CheckpointRetentionSection() {
  const retention = usePrimarySettings((settings) => settings.experimental.checkpointRetention);
  const updateSettings = useUpdatePrimarySettings();
  const patchRetention = useCallback(
    (patch: Partial<typeof retention>) =>
      updateSettings({ experimental: { checkpointRetention: patch } }),
    [updateSettings],
  );

  return (
    <SettingsSection title="Checkpoint retention">
      <SettingsRow
        title="Delete with the thread"
        description="When you delete a thread, immediately remove its hidden checkpoints. Turn this off to keep the history for later inspection."
        control={
          <Switch
            checked={retention.deleteOnThreadDelete}
            onCheckedChange={(checked) =>
              patchRetention({ deleteOnThreadDelete: Boolean(checked) })
            }
            aria-label="Delete checkpoint data when a thread is deleted"
          />
        }
      />
      <SettingsRow
        title="Maximum age"
        description="Remove checkpoint data older than this. Each existing thread always keeps its most recent restore point."
        control={
          <RetentionLimitControl
            value={retention.maxAgeDays}
            min={MIN_CHECKPOINT_RETENTION_DAYS}
            max={MAX_CHECKPOINT_RETENTION_DAYS}
            step={1}
            unitLabel="days"
            ariaLabel="maximum checkpoint age in days"
            onChange={(next) => patchRetention({ maxAgeDays: next === null ? null : next })}
          />
        }
        status={
          retention.maxAgeDays === null
            ? "No age limit."
            : `Default is ${DEFAULT_CHECKPOINT_RETENTION_DAYS} days.`
        }
      />
      <SettingsRow
        title="Maximum size"
        description="Once total checkpoint storage exceeds this, the oldest entries are removed first."
        control={
          <RetentionLimitControl
            value={retention.maxTotalMegabytes}
            min={MIN_CHECKPOINT_RETENTION_MEGABYTES}
            max={MAX_CHECKPOINT_RETENTION_MEGABYTES}
            step={64}
            unitLabel="MB"
            ariaLabel="maximum checkpoint storage in megabytes"
            onChange={(next) => patchRetention({ maxTotalMegabytes: next === null ? null : next })}
          />
        }
        status={
          retention.maxTotalMegabytes === null
            ? "No size limit."
            : `Default is ${DEFAULT_CHECKPOINT_RETENTION_MEGABYTES} MB.`
        }
      />
      <SettingsRow
        title="Apply on server start"
        description="Run the age and size limits automatically when the server starts, in the background."
        control={
          <Switch
            checked={retention.sweepOnStartup}
            onCheckedChange={(checked) => patchRetention({ sweepOnStartup: Boolean(checked) })}
            aria-label="Apply the retention policy on server start"
          />
        }
      />
    </SettingsSection>
  );
}

export function ExperimentalSettingsPanel() {
  return (
    <SettingsPageContainer>
      <CheckpointRetentionSection />
      <CheckpointStorageSection />
    </SettingsPageContainer>
  );
}
