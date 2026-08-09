import { describe, expect, it } from "@effect/vitest";
import * as NodeOS from "node:os";

import {
  makePiContinuationKey,
  makePiEnvironment,
  PI_AGENT_DIR_ENV,
  resolvePiLaunch,
  resolvePiLaunchArgs,
  T3CODE_PI_LAUNCH_ARGS_ENV,
  T3CODE_PI_RUNTIME_MODE_ENV,
} from "./PiLaunch.ts";

const settings = (overrides: {
  readonly binaryPath?: string;
  readonly launchArgs?: string;
  readonly agentDirPath?: string;
}) => ({
  binaryPath: overrides.binaryPath ?? "pi",
  launchArgs: overrides.launchArgs ?? "",
  agentDirPath: overrides.agentDirPath ?? "",
});

describe("resolvePiLaunchArgs", () => {
  it("lets the environment override the configured args", () => {
    expect(resolvePiLaunchArgs("--no-skills", { [T3CODE_PI_LAUNCH_ARGS_ENV]: "--verbose" })).toBe(
      "--verbose",
    );
  });

  it("falls back to the configured args when the env var is blank", () => {
    expect(resolvePiLaunchArgs("--no-skills", { [T3CODE_PI_LAUNCH_ARGS_ENV]: "   " })).toBe(
      "--no-skills",
    );
  });
});

describe("makePiEnvironment", () => {
  it("leaves the environment untouched when no agent dir is configured", () => {
    const base = { PATH: "/usr/bin" };
    expect(makePiEnvironment(settings({}), base)).toBe(base);
  });

  it("isolates the instance through PI_CODING_AGENT_DIR, not HOME", () => {
    const result = makePiEnvironment(settings({ agentDirPath: "/tmp/pi-work" }), {
      HOME: "/Users/dev",
    });
    expect(result[PI_AGENT_DIR_ENV]).toBe("/tmp/pi-work");
    // Overriding HOME would break the macOS keychain lookup for stored creds.
    expect(result.HOME).toBe("/Users/dev");
  });

  it("expands a leading tilde, since spawned processes get no shell expansion", () => {
    const result = makePiEnvironment(settings({ agentDirPath: "~/pi-alt" }), {});
    expect(result[PI_AGENT_DIR_ENV]).toBe(`${NodeOS.homedir()}/pi-alt`);
  });
});

describe("makePiContinuationKey", () => {
  it("groups instances that share an agent directory", () => {
    expect(makePiContinuationKey(settings({ agentDirPath: "/tmp/a" }))).toBe(
      makePiContinuationKey(settings({ agentDirPath: "/tmp/a" })),
    );
    expect(makePiContinuationKey(settings({ agentDirPath: "/tmp/a" }))).not.toBe(
      makePiContinuationKey(settings({ agentDirPath: "/tmp/b" })),
    );
  });

  it("uses a stable key for the default directory", () => {
    expect(makePiContinuationKey(settings({}))).toBe("pi:agent-dir:default");
  });
});

describe("resolvePiLaunch", () => {
  it("always launches RPC mode", () => {
    const launch = resolvePiLaunch({ piSettings: settings({}), environment: {} });
    expect(launch.command).toBe("pi");
    expect(launch.args).toEqual(["--mode", "rpc"]);
  });

  it("honors a configured binary path", () => {
    expect(
      resolvePiLaunch({ piSettings: settings({ binaryPath: "/opt/pi/bin/pi" }), environment: {} })
        .command,
    ).toBe("/opt/pi/bin/pi");
  });

  it("places extensions before user args and caller args last", () => {
    const launch = resolvePiLaunch({
      piSettings: settings({ launchArgs: "--no-skills" }),
      environment: {},
      extensionPaths: ["/ext/a.ts"],
      extraArgs: ["--no-session"],
    });
    expect(launch.args).toEqual([
      "--mode",
      "rpc",
      "--extension",
      "/ext/a.ts",
      "--no-skills",
      "--no-session",
    ]);
  });

  it("tokenizes quoted launch args instead of splitting on every space", () => {
    const launch = resolvePiLaunch({
      piSettings: settings({ launchArgs: '--append-system-prompt "be terse"' }),
      environment: {},
    });
    expect(launch.args).toEqual(["--mode", "rpc", "--append-system-prompt", "be terse"]);
  });
});

describe("resolvePiLaunch runtime mode", () => {
  it("passes the mode to the bundled extension via the environment", () => {
    const launch = resolvePiLaunch({
      piSettings: settings({}),
      environment: {},
      extensionPaths: ["/dist/pi-extension/t3-runtime-mode.ts"],
      runtimeMode: "approval-required",
    });
    expect(launch.args).toEqual([
      "--mode",
      "rpc",
      "--extension",
      "/dist/pi-extension/t3-runtime-mode.ts",
    ]);
    expect(launch.env[T3CODE_PI_RUNTIME_MODE_ENV]).toBe("approval-required");
  });

  it("leaves the mode env var unset when no mode is requested", () => {
    const launch = resolvePiLaunch({ piSettings: settings({}), environment: {} });
    expect(launch.env[T3CODE_PI_RUNTIME_MODE_ENV]).toBeUndefined();
  });
});
