import { describe, expect, it } from "@effect/vitest";
import { ProviderInstanceId } from "@t3tools/contracts";
import * as Effect from "effect/Effect";

import { makeOpenCodeTextGeneration } from "./OpenCodeTextGeneration.ts";
import { type OpenCodeClientFacade, type OpenCodeHostHandle } from "../provider/OpenCodeHost.ts";

describe("OpenCodeTextGeneration", () => {
  it.effect("generates commit message via direct generate.text", () =>
    Effect.gen(function* () {
      const mockClient = {
        generate: {
          text: async () => ({
            text: JSON.stringify({
              subject: "feat: add user authentication",
              body: "Implements user authentication with tests.",
            }),
          }),
        },
      } as unknown as OpenCodeClientFacade;

      const handle: OpenCodeHostHandle = {
        instanceId: ProviderInstanceId.make("opencode-test"),
        client: mockClient,
        isRemote: false,
        databasePath: null,
      };

      const textGen = makeOpenCodeTextGeneration(handle, {
        enabled: true,
        binaryPath: "opencode",
        serverUrl: "",
        serverPassword: "",
        customModels: [],
      });

      const result = yield* textGen.generateCommitMessage({
        cwd: "/tmp",
        branch: "main",
        stagedSummary: "1 file changed",
        stagedPatch: "+ added code",
        modelSelection: {
          instanceId: handle.instanceId,
          model: "anthropic/claude-3-7-sonnet",
        },
      });

      expect(result.subject).toBe("feat: add user authentication");
      expect(result.body).toBe("Implements user authentication with tests.");
    }),
  );

  it.effect("falls back to ephemeral session when direct generate.text is unavailable", () =>
    Effect.gen(function* () {
      const deletedSessions: string[] = [];
      const mockClient = {
        generate: null,
        session: {
          create: async () => ({ id: "ephemeral-1" }),
          prompt: async () => ({
            parts: [
              {
                type: "text",
                text: JSON.stringify({
                  title: "Add authentication system",
                  body: "## Description\nImplements auth.",
                }),
              },
            ],
          }),
          delete: async (params: { sessionID: string }) => {
            deletedSessions.push(params.sessionID);
          },
        },
      } as unknown as OpenCodeClientFacade;

      const handle: OpenCodeHostHandle = {
        instanceId: ProviderInstanceId.make("opencode-test"),
        client: mockClient,
        isRemote: false,
        databasePath: null,
      };

      const textGen = makeOpenCodeTextGeneration(handle, {
        enabled: true,
        binaryPath: "opencode",
        serverUrl: "",
        serverPassword: "",
        customModels: [],
      });

      const result = yield* textGen.generatePrContent({
        cwd: "/tmp",
        baseBranch: "main",
        headBranch: "feature/auth",
        commitSummary: "add auth",
        diffSummary: "1 file changed",
        diffPatch: "+ added pr code",
        modelSelection: {
          instanceId: handle.instanceId,
          model: "anthropic/claude-3-7-sonnet",
        },
      });

      expect(result.title).toBe("Add authentication system");
      expect(result.body).toContain("## Description");
      expect(deletedSessions).toEqual(["ephemeral-1"]);
    }),
  );

  it.effect("generates branch name and thread title", () =>
    Effect.gen(function* () {
      const mockClient = {
        generate: {
          text: async (params: { prompt: string }) => {
            if (params.prompt.includes("key: branch")) {
              return {
                text: JSON.stringify({ branch: "feature/user-auth" }),
              };
            }
            return {
              text: JSON.stringify({ title: "Fix login button styling" }),
            };
          },
        },
      } as unknown as OpenCodeClientFacade;

      const handle: OpenCodeHostHandle = {
        instanceId: ProviderInstanceId.make("opencode-test"),
        client: mockClient,
        isRemote: false,
        databasePath: null,
      };

      const textGen = makeOpenCodeTextGeneration(handle, {
        enabled: true,
        binaryPath: "opencode",
        serverUrl: "",
        serverPassword: "",
        customModels: [],
      });

      const branchResult = yield* textGen.generateBranchName({
        cwd: "/tmp",
        message: "Create branch for auth",
        modelSelection: {
          instanceId: handle.instanceId,
          model: "openai/gpt-4o",
        },
      });
      expect(branchResult.branch).toBe("feature/user-auth");

      const titleResult = yield* textGen.generateThreadTitle({
        cwd: "/tmp",
        message: "Fix login button",
        modelSelection: {
          instanceId: handle.instanceId,
          model: "openai/gpt-4o",
        },
      });
      expect(titleResult.title).toBe("Fix login button styling");
    }),
  );
});
