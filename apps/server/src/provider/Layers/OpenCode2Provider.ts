/**
 * OpenCode2Provider — builds provider status, models, agents, variants, skills,
 * and slash commands for an OpenCode 2 instance.
 *
 * @module provider/Layers/OpenCode2Provider
 */
import {
  type ModelCapabilities,
  type OpenCode2Settings,
  ProviderDriverKind,
  type ServerProviderModel,
  type ServerProviderSkill,
  type ServerProviderSlashCommand,
} from "@t3tools/contracts";
import { createModelCapabilities } from "@t3tools/shared/model";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";

import {
  buildServerProvider,
  COMPACT_SLASH_COMMAND,
  providerModelsFromSettings,
  type ServerProviderDraft,
} from "../providerSnapshot.ts";
import { ProviderDriverError } from "../Errors.ts";
import { type OpenCode2HostHandle } from "../OpenCode2Host.ts";

const OPENCODE2_PRESENTATION = {
  displayName: "OpenCode 2",
  showInteractionModeToggle: false,
  badgeLabel: "Preview",
} as const;

export function titleCaseSlug(value: string): string {
  const segments: Array<string> = [];
  for (const segment of value.split(/[-_/]+/)) {
    if (segment.length > 0) {
      segments.push(segment.charAt(0).toUpperCase() + segment.slice(1));
    }
  }
  return segments.join(" ");
}

function inferDefaultVariant(
  providerID: string,
  variants: ReadonlyArray<string>,
): string | undefined {
  if (variants.length === 1) {
    return variants[0];
  }
  if (providerID === "anthropic" || providerID.startsWith("google")) {
    return variants.includes("high") ? "high" : undefined;
  }
  if (providerID === "openai" || providerID === "opencode") {
    return variants.includes("medium") ? "medium" : variants.includes("high") ? "high" : undefined;
  }
  return variants[0];
}

export function openCode2CapabilitiesForModel(input: {
  readonly providerID: string;
  readonly variants?: ReadonlyArray<{ name: string }> | undefined;
  readonly agents: ReadonlyArray<{ name: string; mode?: string; hidden?: boolean }>;
}): ModelCapabilities {
  const rawVariantValues = (input.variants ?? []).map((v) => v.name);
  const variantValues =
    rawVariantValues.length > 0 ? rawVariantValues : ["low", "medium", "high", "xhigh"];
  const defaultVariant = inferDefaultVariant(input.providerID, variantValues);
  const variantOptions = variantValues.map((value) =>
    defaultVariant === value
      ? { id: value, label: titleCaseSlug(value), isDefault: true as const }
      : { id: value, label: titleCaseSlug(value) },
  );

  const primaryAgents = input.agents.filter(
    (agent) => !agent.hidden && (agent.mode === "primary" || agent.mode === "all"),
  );
  const defaultAgent =
    primaryAgents.find((a) => a.name === "build")?.name ?? primaryAgents[0]?.name;
  const agentOptions = primaryAgents.map((agent) =>
    defaultAgent === agent.name
      ? { id: agent.name, label: titleCaseSlug(agent.name), isDefault: true as const }
      : { id: agent.name, label: titleCaseSlug(agent.name) },
  );

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
              ...(defaultAgent ? { currentValue: defaultAgent } : {}),
            },
          ]
        : []),
    ],
  });
}

const DEFAULT_OPENCODE2_MODEL_CAPABILITIES: ModelCapabilities = createModelCapabilities({
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

export function makePendingOpenCode2Provider(
  config: OpenCode2Settings,
): Effect.Effect<ServerProviderDraft> {
  return Effect.gen(function* () {
    const now = yield* DateTime.now;
    const checkedAt = now.pipe(DateTime.formatIso);
    return {
      ...buildServerProvider({
        presentation: OPENCODE2_PRESENTATION,
        enabled: config.enabled,
        checkedAt,
        models: providerModelsFromSettings(
          [],
          config.customModels,
          DEFAULT_OPENCODE2_MODEL_CAPABILITIES,
        ),
        slashCommands: [COMPACT_SLASH_COMMAND],
        skills: [],
        probe: {
          installed: true,
          version: "2.0.0-preview",
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

export function checkOpenCode2ProviderStatus(
  hostHandle: OpenCode2HostHandle,
  config: OpenCode2Settings,
  cwd: string,
): Effect.Effect<ServerProviderDraft> {
  return Effect.gen(function* () {
    const now = yield* DateTime.now;
    const checkedAt = now.pipe(DateTime.formatIso);

    if (!config.enabled) {
      return {
        ...buildServerProvider({
          presentation: OPENCODE2_PRESENTATION,
          enabled: false,
          checkedAt,
          models: providerModelsFromSettings(
            [],
            config.customModels,
            DEFAULT_OPENCODE2_MODEL_CAPABILITIES,
          ),
          slashCommands: [COMPACT_SLASH_COMMAND],
          skills: [],
          probe: {
            installed: true,
            version: "2.0.0-preview",
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
        const [modelsRes, providersRes, agentsRes, commandsRes, skillsRes] = await Promise.all([
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
          variants?: Array<{ name: string }>;
        }> = [];

        for (const model of (modelsRes as { data?: any[] }).data ?? []) {
          if (model?.id) {
            const providerID = model.providerID || model.id.split("/")[0] || "opencode";
            const subProvider =
              providerNames.get(providerID) || (providerID ? titleCaseSlug(providerID) : undefined);
            models.push({
              id: model.id,
              name: model.name || titleCaseSlug(model.id),
              providerID,
              subProvider,
              variants: model.variants,
            });
          }
        }

        return {
          models,
          agents: (agentsRes as { data?: unknown[] }).data ?? [],
          commands: (commandsRes as { data?: unknown[] }).data ?? [],
          skills: (skillsRes as { data?: unknown[] }).data ?? [],
          failed: false,
        };
      },
      catch: (cause) =>
        new ProviderDriverError({
          driver: ProviderDriverKind.make("opencode2"),
          instanceId: hostHandle.instanceId,
          detail: String(cause),
          cause,
        }),
    }).pipe(
      Effect.orElseSucceed(() => ({
        models: [],
        agents: [],
        commands: [],
        skills: [],
        failed: true,
      })),
    );

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
        variants?: Array<{ name: string }>;
      }>
    ).map((model) => ({
      slug: model.id,
      name: model.name || titleCaseSlug(model.id),
      ...(model.subProvider ? { subProvider: model.subProvider } : {}),
      isCustom: false,
      capabilities: openCode2CapabilitiesForModel({
        providerID: model.providerID,
        variants: model.variants,
        agents: availableAgents,
      }),
    }));

    const customModels = providerModelsFromSettings(
      probedModels,
      config.customModels,
      DEFAULT_OPENCODE2_MODEL_CAPABILITIES,
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
        presentation: OPENCODE2_PRESENTATION,
        enabled: true,
        checkedAt,
        models,
        slashCommands,
        skills,
        probe: {
          installed: true,
          version: "2.0.0-preview",
          status: inventory.failed ? "warning" : "ready",
          auth: { status: "unknown" },
          ...(inventory.failed
            ? { message: "Could not reach OpenCode 2 host to discover models and tools." }
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
