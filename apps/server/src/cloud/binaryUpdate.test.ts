import * as NodeServices from "@effect/platform-node/NodeServices";
import { expect, it } from "@effect/vitest";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import { HttpClient, HttpClientResponse } from "effect/unstable/http";
import * as ChildProcessSpawner from "effect/unstable/process/ChildProcessSpawner";

import { HostProcessEnvironment } from "@t3tools/shared/hostProcess";

import * as ProcessRunner from "../processRunner.ts";
import { BOOT_SERVICE_UNIT_ENV } from "./bootService.ts";
import type { ServerBinaryIdentity } from "./binaryRuntime.ts";
import {
  prepareServerBinaryUpdate,
  resolveRestartMethod,
  serverBinaryDownloadUrl,
} from "./binaryUpdate.ts";

const REPLACEMENT_BYTES = "#!/replacement\n";

const makeRunner = (options: {
  readonly exitCode?: number;
  readonly stdout?: string;
  readonly onRun?: (command: string) => void;
}) =>
  ProcessRunner.ProcessRunner.of({
    run: (input) =>
      Effect.sync(() => {
        options.onRun?.(input.command);
        return {
          stdout: options.stdout ?? "t3 v1.2.3",
          stderr: "",
          code: ChildProcessSpawner.ExitCode(options.exitCode ?? 0),
          timedOut: false,
          stdoutTruncated: false,
          stderrTruncated: false,
        };
      }),
  });

const makeHttpClient = (body: string | null) =>
  HttpClient.make((request) =>
    body === null
      ? Effect.succeed(
          HttpClientResponse.fromWeb(request, new Response("not found", { status: 404 })),
        )
      : Effect.succeed(HttpClientResponse.fromWeb(request, new Response(body, { status: 200 }))),
  );

it("builds the release download URL from the identity and target version", () => {
  expect(
    serverBinaryDownloadUrl({ repo: "owner/repo", version: "1.2.3", target: "linux-arm64" }),
  ).toBe("https://github.com/owner/repo/releases/download/v1.2.3/s5code-server-1.2.3-linux-arm64");
});

it("uses systemd when the boot-service unit env is present, self-exec otherwise", () => {
  expect(resolveRestartMethod({ [BOOT_SERVICE_UNIT_ENV]: "t3code.service" })).toBe("systemd");
  expect(resolveRestartMethod({ [BOOT_SERVICE_UNIT_ENV]: "   " })).toBe("self-exec");
  // systemd sets INVOCATION_ID for every unit invocation, which covers
  // hand-written units that never ran `t3 service install`.
  expect(resolveRestartMethod({ INVOCATION_ID: "3f2a9c8e0d1b4f5e9a6b7c8d9e0f1a2b" })).toBe(
    "systemd",
  );
  expect(resolveRestartMethod({})).toBe("self-exec");
});

it.layer(NodeServices.layer)("binary self-update", (it) => {
  const withBinary = <A, E, R>(
    body: (input: {
      readonly identity: ServerBinaryIdentity;
      readonly executablePath: string;
      readonly fs: FileSystem.FileSystem;
      readonly directory: string;
    }) => Effect.Effect<A, E, R>,
  ) =>
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const path = yield* Path.Path;
      const directory = yield* fs.makeTempDirectoryScoped({ prefix: "t3-binary-update-test-" });
      const executablePath = path.join(directory, "s5code-server");
      yield* fs.writeFileString(executablePath, "#!/original\n");
      yield* fs.chmod(executablePath, 0o755);
      return yield* body({
        identity: { target: "linux-x64", repo: "owner/repo", executablePath },
        executablePath,
        fs,
        directory,
      });
    });

  it.effect("downloads, validates and atomically replaces the running executable", () =>
    withBinary(({ identity, executablePath, fs }) =>
      Effect.gen(function* () {
        const stages: ReadonlyArray<string> = [];
        const validated: string[] = [];
        const plan = yield* prepareServerBinaryUpdate({
          identity,
          targetVersion: "1.2.3",
          reportProgress: (stage) =>
            Effect.sync(() => {
              (stages as string[]).push(stage);
            }),
        }).pipe(
          Effect.provideService(HttpClient.HttpClient, makeHttpClient(REPLACEMENT_BYTES)),
          Effect.provideService(
            ProcessRunner.ProcessRunner,
            makeRunner({ onRun: (command) => validated.push(command) }),
          ),
          Effect.provideService(HostProcessEnvironment, {}),
        );

        expect(stages).toEqual(["downloading", "installing"]);
        expect(plan.targetVersion).toBe("1.2.3");
        expect(plan.restart).toBe("self-exec");
        // Validation runs the staged copy, never the live executable.
        expect(validated).toHaveLength(1);
        expect(validated[0]).not.toBe(executablePath);
        // The replacement is in place, still executable, and no staged file is left.
        expect(yield* fs.readFileString(executablePath)).toBe(REPLACEMENT_BYTES);
        const info = yield* fs.stat(executablePath);
        expect(info.mode & 0o111).not.toBe(0);
        expect(yield* fs.exists(plan.stagedPath)).toBe(false);
      }),
    ),
  );

  it.effect("reports systemd restart when running under the boot service unit", () =>
    withBinary(({ identity }) =>
      Effect.gen(function* () {
        const plan = yield* prepareServerBinaryUpdate({
          identity,
          targetVersion: "1.2.3",
          reportProgress: () => Effect.void,
        }).pipe(
          Effect.provideService(HttpClient.HttpClient, makeHttpClient(REPLACEMENT_BYTES)),
          Effect.provideService(ProcessRunner.ProcessRunner, makeRunner({})),
          Effect.provideService(HostProcessEnvironment, {
            [BOOT_SERVICE_UNIT_ENV]: "t3code.service",
          }),
        );
        expect(plan.restart).toBe("systemd");
      }),
    ),
  );

  it.effect("keeps the running binary when the release asset is missing", () =>
    withBinary(({ identity, executablePath, fs }) =>
      Effect.gen(function* () {
        const error = yield* prepareServerBinaryUpdate({
          identity,
          targetVersion: "9.9.9",
          reportProgress: () => Effect.void,
        }).pipe(
          Effect.provideService(HttpClient.HttpClient, makeHttpClient(null)),
          Effect.provideService(ProcessRunner.ProcessRunner, makeRunner({})),
          Effect.provideService(HostProcessEnvironment, {}),
          Effect.flip,
        );
        expect(error.step).toBe("download");
        expect(yield* fs.readFileString(executablePath)).toBe("#!/original\n");
      }),
    ),
  );

  it.effect("keeps the running binary when the download reports a different version", () =>
    withBinary(({ identity, executablePath, fs, directory }) =>
      Effect.gen(function* () {
        const error = yield* prepareServerBinaryUpdate({
          identity,
          targetVersion: "1.2.3",
          reportProgress: () => Effect.void,
        }).pipe(
          Effect.provideService(HttpClient.HttpClient, makeHttpClient(REPLACEMENT_BYTES)),
          Effect.provideService(ProcessRunner.ProcessRunner, makeRunner({ stdout: "t3 v0.9.0" })),
          Effect.provideService(HostProcessEnvironment, {}),
          Effect.flip,
        );
        expect(error.step).toBe("validate");
        expect(yield* fs.readFileString(executablePath)).toBe("#!/original\n");
        // No staged artifact is left beside the live binary.
        const entries = yield* fs.readDirectory(directory);
        expect(entries).toEqual(["s5code-server"]);
      }),
    ),
  );

  it.effect("keeps the running binary when the download does not run", () =>
    withBinary(({ identity, executablePath, fs }) =>
      Effect.gen(function* () {
        const error = yield* prepareServerBinaryUpdate({
          identity,
          targetVersion: "1.2.3",
          reportProgress: () => Effect.void,
        }).pipe(
          Effect.provideService(HttpClient.HttpClient, makeHttpClient(REPLACEMENT_BYTES)),
          Effect.provideService(ProcessRunner.ProcessRunner, makeRunner({ exitCode: 127 })),
          Effect.provideService(HostProcessEnvironment, {}),
          Effect.flip,
        );
        expect(error.step).toBe("validate");
        expect(yield* fs.readFileString(executablePath)).toBe("#!/original\n");
      }),
    ),
  );

  it.effect("includes process output when the downloaded binary exits non-zero", () =>
    withBinary(({ identity }) =>
      Effect.gen(function* () {
        const error = yield* prepareServerBinaryUpdate({
          identity,
          targetVersion: "1.2.3",
          reportProgress: () => Effect.void,
        }).pipe(
          Effect.provideService(HttpClient.HttpClient, makeHttpClient(REPLACEMENT_BYTES)),
          Effect.provideService(
            ProcessRunner.ProcessRunner,
            ProcessRunner.ProcessRunner.of({
              run: () =>
                Effect.succeed({
                  stdout: "",
                  stderr: "error: No such built-in module: node:sqlite\n",
                  code: ChildProcessSpawner.ExitCode(1),
                  timedOut: false,
                  stdoutTruncated: false,
                  stderrTruncated: false,
                }),
            }),
          ),
          Effect.provideService(HostProcessEnvironment, {}),
          Effect.flip,
        );
        expect(error.step).toBe("validate");
        expect(error.detail).toContain("node:sqlite");
      }),
    ),
  );
});
