import * as NodeServices from "@effect/platform-node/NodeServices";
import { describe, expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";

import {
  hasDeployChanges,
  missingRelayPublicConfigFields,
  publicConfigFromOutput,
  reconcileRootEnvPublicConfig,
  RelayDeployError,
  RelayDeployPublicConfigUnavailableError,
  serializeGithubOutput,
} from "./deploy.ts";

describe("RelayDeployError", () => {
  it("reports the incomplete state source, stage, and missing fields", () => {
    const missingFields = missingRelayPublicConfigFields({});
    const error = new RelayDeployError({
      source: "alchemy_state",
      stage: "production",
      missingFields,
    });

    expect(error).toMatchObject({
      source: "alchemy_state",
      stage: "production",
      missingFields: ["url"],
    });
    expect(error.message).toBe(
      "Relay deploy output from 'alchemy_state' for stage 'production' is missing required public config fields: url",
    );
  });

  it("distinguishes deploy results that do not produce public config", () => {
    const error = new RelayDeployPublicConfigUnavailableError({
      result: "dry-run",
      stage: "production",
      outputPath: "/tmp/relay-client.env",
    });

    expect(error.message).toBe(
      "Relay deploy result 'dry-run' for stage 'production' did not produce public config required by GitHub environment output '/tmp/relay-client.env'.",
    );
  });
});

describe("hasDeployChanges", () => {
  it("detects resource, binding, and deletion changes", () => {
    expect(hasDeployChanges({ resources: {}, deletions: {} } as never)).toBe(false);
    expect(
      hasDeployChanges({
        resources: {
          api: { action: "create", bindings: [] },
        },
        deletions: {},
      } as never),
    ).toBe(true);
    expect(
      hasDeployChanges({
        resources: {
          api: { action: "noop", bindings: [{ action: "update" }] },
        },
        deletions: {},
      } as never),
    ).toBe(true);
    expect(
      hasDeployChanges({
        resources: {},
        deletions: {
          api: { action: "delete", bindings: [] },
        },
      } as never),
    ).toBe(true);
  });
});

describe("reconcileRootEnvPublicConfig", () => {
  const config = {
    relayUrl: "https://relay.example.test",
  } as const;

  it("adds the complete local client config", () => {
    expect(reconcileRootEnvPublicConfig("", config)).toBe(
      ["T3CODE_RELAY_URL=https://relay.example.test", ""].join("\n"),
    );
  });

  it("replaces stale values while preserving unrelated entries", () => {
    expect(
      reconcileRootEnvPublicConfig(
        [
          "T3CODE_CLERK_PUBLISHABLE_KEY=pk_test_example",
          "T3CODE_RELAY_URL=https://old.example.test",
          "",
        ].join("\n"),
        config,
      ),
    ).toBe(
      [
        "T3CODE_CLERK_PUBLISHABLE_KEY=pk_test_example",
        "T3CODE_RELAY_URL=https://relay.example.test",
        "",
      ].join("\n"),
    );
  });
});

describe("serializeGithubOutput", () => {
  it("serializes relay deploy metadata for GitHub Actions outputs", () => {
    expect(
      serializeGithubOutput({
        changed: false,
        result: "noop",
        relay_url: "https://relay.example.test",
      }),
    ).toBe("changed=false\nresult=noop\nrelay_url=https://relay.example.test\n");
  });
});

describe("release workflow tracing config propagation", () => {
  it.effect("ships no relay tracing plumbing of its own", () =>
    Effect.gen(function* () {
      const fileSystem = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const workflowPath = yield* path.fromFileUrl(
        new URL("../../../.github/workflows/s5-release.yml", import.meta.url),
      );
      const workflow = yield* fileSystem.readFileString(workflowPath);

      // This fork deploys its own relay (see .github/workflows/s5-deploy-relay.yml
      // and infra/relay), so the release bakes in that relay's public config
      // as constants and carries no tracing artifact or masked token output.
      expect(workflow).not.toContain("client_tracing_token:");
      expect(workflow).not.toContain("needs.relay_public_config.outputs.client_tracing_token");
      expect(workflow).not.toContain("relay-client-tracing");
      expect(workflow).not.toContain("--read-state");
      expect(workflow).toContain("relay_url=https://relay.s5code.dpdns.org");
    }).pipe(Effect.provide(NodeServices.layer)),
  );
});

describe("publicConfigFromOutput", () => {
  it("reads the relay URL from persisted Alchemy output", () => {
    expect(
      publicConfigFromOutput({
        url: "https://relay.example.test",
      }),
    ).toEqual({
      relayUrl: "https://relay.example.test",
    });
  });

  it("rejects incomplete stack output", () => {
    expect(publicConfigFromOutput({})).toBeNull();
  });
});
