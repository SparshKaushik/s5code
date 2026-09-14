/**
 * OpenCodeProvider — builds provider status, models, agents, variants, skills,
 * and slash commands for an OpenCode instance.
 *
 * @module provider/Layers/OpenCodeProvider
 */
import {
  type ModelCapabilities,
  type OpenCodeSettings,
  ProviderDriverKind,
  type ServerProviderModel,
  type ServerProviderSkill,
  type ServerProviderSlashCommand,
} from "@t3tools/contracts";
import { createModelCapabilities } from "@t3tools/shared/model";
import { parseSemver } from "@t3tools/shared/semver";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";

import {
  buildServerProvider,
  COMPACT_SLASH_COMMAND,
  providerModelsFromSettings,
  type ServerProviderDraft,
} from "../providerSnapshot.ts";
import { ProviderDriverError } from "../Errors.ts";
import { type OpenCodeHostHandle } from "../OpenCodeHost.ts";

const OPENCODE_PRESENTATION = {
  displayName: "OpenCode",
  showInteractionModeToggle: false,
} as const;

export function titleCaseSlug(value: string): string {
  if (value.toLowerCase() === "openai") return "OpenAI";
  const segments: Array<string> = [];
  for (const segment of value.split(/[-_/]+/)) {
    if (segment.length > 0) {
      segments.push(segment.charAt(0).toUpperCase() + segment.slice(1));
    }
  }
  return segments.join(" ");
}

export function formatVariantLabel(value: string): string {
  switch (value.toLowerCase()) {
    case "xhigh":
      return "Extra High";
    case "none":
      return "None";
    default:
      return titleCaseSlug(value);
  }
}

export function parseModelVariants(rawVariants: unknown): Array<{ id: string }> | undefined {
  if (!Array.isArray(rawVariants)) return undefined;
  const result: Array<{ id: string }> = [];
  for (const item of rawVariants) {
    if (typeof item === "string" && item.trim().length > 0) {
      result.push({ id: item.trim() });
    } else if (item && typeof item === "object" && typeof (item as any).id === "string") {
      const id = (item as any).id.trim();
      if (id.length > 0) {
        result.push({ id });
      }
    }
  }
  return result.length > 0 ? result : undefined;
}

function inferDefaultVariant(
  providerID: string,
  variants: ReadonlyArray<string>,
): string | undefined {
  if (variants.length === 0) {
    return undefined;
  }
  if (variants.length === 1) {
    return variants[0];
  }
  if (providerID === "anthropic" || providerID.startsWith("google") || providerID === "kiro") {
    return variants.includes("high")
      ? "high"
      : variants.includes("medium")
        ? "medium"
        : (variants.find((v) => v !== "none") ?? variants[0]);
  }
  if (providerID === "openai" || providerID === "opencode") {
    return variants.includes("medium")
      ? "medium"
      : variants.includes("high")
        ? "high"
        : (variants.find((v) => v !== "none") ?? variants[0]);
  }
  return variants.find((v) => v !== "none") ?? variants[0];
}

export function openCodeCapabilitiesForModel(input: {
  readonly providerID: string;
  readonly variants?: ReadonlyArray<{ id: string }> | undefined;
  readonly agents: ReadonlyArray<{ id?: string; name: string; mode?: string; hidden?: boolean }>;
}): ModelCapabilities {
  const rawVariantValues = (input.variants ?? []).map((v) => v.id);
  const variantValues = rawVariantValues;
  const defaultVariant = inferDefaultVariant(input.providerID, variantValues);
  const variantOptions = variantValues.map((value) =>
    defaultVariant === value
      ? { id: value, label: formatVariantLabel(value), isDefault: true as const }
      : { id: value, label: formatVariantLabel(value) },
  );

  const getAgentId = (agent: { id?: string; name: string }) => agent.id || agent.name.toLowerCase();
  const getAgentLabel = (agent: { id?: string; name: string }) =>
    agent.name || titleCaseSlug(agent.id || "");

  const primaryAgents = input.agents.filter(
    (agent) => !agent.hidden && (agent.mode === "primary" || agent.mode === "all"),
  );
  const defaultAgentId =
    primaryAgents.map(getAgentId).find((id) => id === "build") ??
    (primaryAgents[0] ? getAgentId(primaryAgents[0]) : undefined);
  const agentOptions = primaryAgents.map((agent) => {
    const id = getAgentId(agent);
    const label = getAgentLabel(agent);
    return id === defaultAgentId ? { id, label, isDefault: true as const } : { id, label };
  });

  return createModelCapabilities({
    optionDescriptors: [
      ...(variantOptions.length > 0
        ? [
            {
              id: "variant",
              label: "Reasoning",
              type: "select" as const,
              options: variantOptions,
              ...(defaultVariant ? { currentValue: defaultVariant } : {}),
            },
          ]
        : []),
      ...(agentOptions.length > 0
        ? [
            {
              id: "agent",
              label: "Agent",
              type: "select" as const,
              options: agentOptions,
              ...(defaultAgentId ? { currentValue: defaultAgentId } : {}),
            },
          ]
        : []),
    ],
  });
}

const DEFAULT_OPENCODE_MODEL_CAPABILITIES: ModelCapabilities = createModelCapabilities({
  optionDescriptors: [
    {
      id: "variant",
      label: "Reasoning",
      type: "select",
      options: [
        { id: "low", label: "Low" },
        { id: "medium", label: "Medium", isDefault: true },
        { id: "high", label: "High" },
        { id: "xhigh", label: "Extra High" },
      ],
      currentValue: "medium",
    },
    {
      id: "agent",
      label: "Agent",
      type: "select",
      options: [
        { id: "build", label: "Build", isDefault: true },
        { id: "plan", label: "Plan" },
      ],
      currentValue: "build",
    },
  ],
});

export const MINIMUM_OPENCODE_VERSION = "2.0.0";

export function isOpenCodeVersionSupported(version: string | null | undefined): boolean {
  if (!version) return false;
  const clean = version.trim().replace(/^v/, "");
  const parsed = parseSemver(clean);
  if (!parsed) return false;
  if (parsed.major >= 2) return true;
  // Allow development / beta builds (e.g. 0.0.0-beta-19425)
  if (parsed.major === 0 && parsed.minor === 0 && parsed.patch === 0) return true;
  return false;
}

export function makePendingOpenCodeProvider(
  config: OpenCodeSettings,
): Effect.Effect<ServerProviderDraft> {
  return Effect.gen(function* () {
    const now = yield* DateTime.now;
    const checkedAt = now.pipe(DateTime.formatIso);
    return {
      ...buildServerProvider({
        presentation: OPENCODE_PRESENTATION,
        enabled: config.enabled,
        checkedAt,
        models: providerModelsFromSettings(
          [],
          config.customModels,
          DEFAULT_OPENCODE_MODEL_CAPABILITIES,
        ),
        slashCommands: [COMPACT_SLASH_COMMAND],
        skills: [],
        probe: {
          installed: true,
          version: null,
          status: "ready",
          auth: { status: "unknown" },
        },
      }),
      supportsConversationRollback: true,
      supportsConversationFork: true,
      supportsInboxSteering: true,
      supportsInboxQueueing: true,
      supportsTextGeneration: true,
    };
  });
}

export function checkOpenCodeProviderStatus(
  hostHandle: OpenCodeHostHandle,
  config: OpenCodeSettings,
  cwd: string,
): Effect.Effect<ServerProviderDraft> {
  return Effect.gen(function* () {
    const now = yield* DateTime.now;
    const checkedAt = now.pipe(DateTime.formatIso);

    if (!config.enabled) {
      return {
        ...buildServerProvider({
          presentation: OPENCODE_PRESENTATION,
          enabled: false,
          checkedAt,
          models: providerModelsFromSettings(
            [],
            config.customModels,
            DEFAULT_OPENCODE_MODEL_CAPABILITIES,
          ),
          slashCommands: [COMPACT_SLASH_COMMAND],
          skills: [],
          probe: {
            installed: true,
            version: null,
            status: "ready",
            auth: { status: "unknown" },
          },
        }),
        supportsConversationRollback: true,
        supportsConversationFork: true,
        supportsInboxSteering: true,
        supportsInboxQueueing: true,
        supportsTextGeneration: true,
      };
    }

    const inventory = yield* Effect.tryPromise({
      try: async () => {
        const [healthRes, modelsRes, providersRes, agentsRes, commandsRes, skillsRes] =
          await Promise.all([
            hostHandle.client.health.get().catch((cause) => ({ cause, healthy: false as const })),
            hostHandle.client.model
              .list({ location: { directory: cwd } })
              .catch(() => ({ data: [] })),
            hostHandle.client.provider
              .list({ location: { directory: cwd } })
              .catch(() => ({ data: [] })),
            hostHandle.client.agent
              .list({ location: { directory: cwd } })
              .catch(() => ({ data: [] })),
            hostHandle.client.command
              .list({ location: { directory: cwd } })
              .catch(() => ({ data: [] })),
            hostHandle.client.skill
              .list({ location: { directory: cwd } })
              .catch(() => ({ data: [] })),
          ]);

        const probedVersion =
          healthRes && "version" in healthRes && typeof healthRes.version === "string"
            ? healthRes.version
            : null;

        const providerNames = new Map<string, string>();
        for (const prov of (providersRes as { data?: Array<{ id?: string; name?: string }> })
          .data ?? []) {
          if (prov?.id && prov?.name) {
            providerNames.set(prov.id, prov.name);
          }
        }

        const models: Array<{
          id: string;
          name: string;
          providerID: string;
          subProvider?: string;
          variants?: Array<{ id: string }>;
        }> = [];

        for (const model of (modelsRes as { data?: any[] }).data ?? []) {
          if (model?.id) {
            const providerID = model.providerID || model.id.split("/")[0] || "opencode";
            const subProvider =
              providerNames.get(providerID) || (providerID ? titleCaseSlug(providerID) : undefined);
            const slug = model.id.startsWith(`${providerID}/`)
              ? model.id
              : `${providerID}/${model.id}`;
            const variants = parseModelVariants(model.variants);
            models.push({
              id: slug,
              name: model.name || titleCaseSlug(model.id),
              providerID,
              ...(subProvider ? { subProvider } : {}),
              ...(variants ? { variants } : {}),
            });
          }
        }

        return {
          version: probedVersion,
          models,
          agents: (agentsRes as { data?: unknown[] }).data ?? [],
          commands: (commandsRes as { data?: unknown[] }).data ?? [],
          skills: (skillsRes as { data?: unknown[] }).data ?? [],
          failed: probedVersion === null,
        };
      },
      catch: (cause) =>
        new ProviderDriverError({
          driver: ProviderDriverKind.make("opencode"),
          instanceId: hostHandle.instanceId,
          detail: String(cause),
          cause,
        }),
    }).pipe(
      Effect.orElseSucceed(() => ({
        version: null,
        models: [],
        agents: [],
        commands: [],
        skills: [],
        failed: true,
      })),
    );

    if (inventory.version !== null && !isOpenCodeVersionSupported(inventory.version)) {
      return {
        ...buildServerProvider({
          presentation: OPENCODE_PRESENTATION,
          enabled: true,
          checkedAt,
          models: [],
          slashCommands: [],
          skills: [],
          probe: {
            installed: true,
            version: inventory.version,
            status: "error",
            auth: { status: "unknown" },
            message: `OpenCode v${inventory.version} is not supported. T3 Code requires OpenCode v${MINIMUM_OPENCODE_VERSION} or newer.`,
          },
        }),
        supportsConversationRollback: true,
        supportsConversationFork: true,
        supportsInboxSteering: true,
        supportsInboxQueueing: true,
        supportsTextGeneration: true,
      };
    }

    const availableAgents = inventory.agents as Array<{
      name: string;
      mode?: string;
      hidden?: boolean;
    }>;

    const probedModels: Array<ServerProviderModel> = (
      inventory.models as Array<{
        id: string;
        name: string;
        providerID: string;
        subProvider?: string;
        variants?: Array<{ id: string }>;
      }>
    ).map((model) => ({
      slug: model.id,
      name: model.name || titleCaseSlug(model.id),
      ...(model.subProvider ? { subProvider: model.subProvider } : {}),
      isCustom: false,
      capabilities: openCodeCapabilitiesForModel({
        providerID: model.providerID,
        variants: model.variants,
        agents: availableAgents,
      }),
    }));

    const customModels = providerModelsFromSettings(
      probedModels,
      config.customModels,
      DEFAULT_OPENCODE_MODEL_CAPABILITIES,
    );
    const models = customModels;

    const slashCommands: Array<ServerProviderSlashCommand> = [
      COMPACT_SLASH_COMMAND,
      ...(inventory.commands as Array<{ name: string; description?: string }>).map((cmd) => ({
        name: cmd.name,
        ...(cmd.description ? { description: cmd.description } : {}),
      })),
    ];

    const skills: Array<ServerProviderSkill> = (
      inventory.skills as Array<{
        name: string;
        description?: string;
        location: string;
        slash?: boolean;
        autoinvoke?: boolean;
      }>
    ).map((sk) => ({
      name: sk.name,
      description: sk.description ?? sk.name,
      path: sk.location,
      enabled: true,
      displayName: titleCaseSlug(sk.name),
      userInvocationOnly: false,
      userInvocable: sk.slash !== false,
    }));

    return {
      ...buildServerProvider({
        presentation: OPENCODE_PRESENTATION,
        enabled: true,
        checkedAt,
        models,
        slashCommands,
        skills,
        probe: {
          installed: !hostHandle.isRemote || inventory.version !== null,
          version: inventory.version,
          status: inventory.failed ? "error" : "ready",
          auth: { status: "unknown" },
          ...(inventory.failed
            ? { message: "Could not reach OpenCode host to discover models and tools." }
            : {}),
        },
      }),
      supportsConversationRollback: true,
      supportsConversationFork: true,
      supportsInboxSteering: true,
      supportsInboxQueueing: true,
      supportsTextGeneration: true,
    };
  });
}
