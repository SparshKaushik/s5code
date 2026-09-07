// CommonJS dependencies bundled or loaded inside an ES module environment
// (such as TypeScript, write-file-atomic, or @opencode-ai packages) expect
// __filename and __dirname to be defined. In ESM under Node 22+, referencing
// them can trigger ERR_AMBIGUOUS_MODULE_SYNTAX if top-level await is present.
if (typeof (globalThis as any).__filename === "undefined") {
  (globalThis as any).__filename = import.meta.filename ?? "";
}
if (typeof (globalThis as any).__dirname === "undefined") {
  (globalThis as any).__dirname = import.meta.dirname ?? "";
}
if (process.env.OPENCODE_TREE_SITTER_WASM_PATH === undefined) {
  process.env.OPENCODE_TREE_SITTER_WASM_PATH = "";
}
if (process.env.OPENCODE_TREE_SITTER_BASH_WASM_PATH === undefined) {
  process.env.OPENCODE_TREE_SITTER_BASH_WASM_PATH = "";
}
if (process.env.OPENCODE_TREE_SITTER_POWERSHELL_WASM_PATH === undefined) {
  process.env.OPENCODE_TREE_SITTER_POWERSHELL_WASM_PATH = "";
}
if (process.env.OPENCODE_PHOTON_WASM_PATH === undefined) {
  process.env.OPENCODE_PHOTON_WASM_PATH = "";
}
if (process.env.OPENCODE_NODE_PTY_PATH === undefined) {
  process.env.OPENCODE_NODE_PTY_PATH = "node-pty";
}

import * as NodeRuntime from "@effect/platform-node/NodeRuntime";
import * as NodeServices from "@effect/platform-node/NodeServices";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { Argument, Command } from "effect/unstable/cli";
import * as CliError from "effect/unstable/cli/CliError";

import * as NetService from "@t3tools/shared/Net";
import packageJson from "../package.json" with { type: "json" };
import { authCommand } from "./cli/auth.ts";
import { appCommand } from "./cli/app.ts";
import { connectCommand } from "./cli/connect.ts";
import { migrateDataCommand } from "./cli/migrate-data.ts";
import { pairCommand } from "./cli/pair.ts";
import { hasCloudPublicConfig } from "./cloud/publicConfig.ts";
import { sharedServerCommandFlags } from "./cli/config.ts";
import { isEntrypoint } from "./entrypoint.ts";
import { projectCommand } from "./cli/project.ts";
import { runServerCommand, serveCommand, startCommand } from "./cli/server.ts";
import { serviceCommand } from "./cli/service.ts";
import { servicePreflightCommand } from "./cli/servicePreflight.ts";
import { themeCommand } from "./cli/theme.ts";
import { triageCommand } from "./cli/triage.ts";

const CliRuntimeLayer = Layer.mergeAll(NodeServices.layer, NetService.layer);

const connectPublicConfigMissingMessage =
  "T3 Connect commands are unavailable: this build is missing T3 Connect public configuration.";

class ConnectPublicConfigMissingError extends CliError.UserError {
  override get message() {
    return connectPublicConfigMissingMessage;
  }
}

const connectUnavailableCommand = Command.make("connect", {
  command: Argument.string("command").pipe(Argument.variadic),
}).pipe(
  Command.withDescription("T3 Connect is unavailable in builds without public configuration."),
  Command.withHidden,
  Command.withHandler(() =>
    Effect.fail(
      new CliError.ShowHelp({
        commandPath: ["t3", "connect"],
        errors: [new ConnectPublicConfigMissingError({ cause: connectPublicConfigMissingMessage })],
      }),
    ),
  ),
);

export const makeCli = ({ cloudEnabled = hasCloudPublicConfig } = {}) =>
  Command.make("t3", { ...sharedServerCommandFlags }).pipe(
    Command.withDescription("Run the S5 Code server."),
    Command.withHandler((flags) => runServerCommand(flags)),
    Command.withSubcommands([
      startCommand,
      serveCommand,
      appCommand,
      pairCommand,
      authCommand,
      projectCommand,
      serviceCommand,
      servicePreflightCommand,
      migrateDataCommand,
      themeCommand,
      triageCommand,
      cloudEnabled ? connectCommand : connectUnavailableCommand,
    ]),
  );

export const cli = makeCli();

if (
  isEntrypoint({
    moduleUrl: import.meta.url,
    entryPath: process.argv[1],
    runtimeMain: import.meta.main,
  })
) {
  Command.run(cli, { version: packageJson.version }).pipe(
    Effect.scoped,
    Effect.provide(CliRuntimeLayer),
    NodeRuntime.runMain,
  );
}
