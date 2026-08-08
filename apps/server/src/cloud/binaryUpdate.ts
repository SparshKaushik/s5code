import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Scope from "effect/Scope";
import { HttpClient, HttpClientRequest, HttpClientResponse } from "effect/unstable/http";
import { ChildProcess, ChildProcessSpawner } from "effect/unstable/process";

import { HostProcessEnvironment } from "@t3tools/shared/hostProcess";

import * as ProcessRunner from "../processRunner.ts";
import { BOOT_SERVICE_UNIT_ENV } from "./bootService.ts";
import { serverBinaryAssetName, type ServerBinaryIdentity } from "./binaryRuntime.ts";

/**
 * Self-update for the precompiled single-file server binary.
 *
 * The binary has no npm tree to install into, so the launcher-backed
 * pinned-runtime flow does not apply. Instead we download the release asset for
 * this exact version and target, prove it runs and reports the version we asked
 * for, then atomically replace the running executable and restart.
 *
 * The swap is a same-directory `rename`, which is atomic on POSIX and legal
 * while the old image is still executing (the running process keeps its open
 * inode; only the directory entry moves).
 */

const DOWNLOAD_TIMEOUT = Duration.minutes(10);
const VALIDATE_TIMEOUT = Duration.seconds(30);

/** How the replacement image gets to run. */
export type ServerBinaryRestartMethod = "systemd" | "self-exec";

export class ServerBinaryUpdateError extends Schema.TaggedErrorClass<ServerBinaryUpdateError>()(
  "ServerBinaryUpdateError",
  {
    step: Schema.Literals([
      "resolve-asset",
      "download",
      "write",
      "validate",
      "activate",
      "restart",
    ]),
    detail: Schema.String,
    cause: Schema.optional(Schema.Defect()),
  },
) {
  override get message(): string {
    return this.detail;
  }
}

const fail = (
  step: ServerBinaryUpdateError["step"],
  detail: string,
  cause?: unknown,
): ServerBinaryUpdateError =>
  cause === undefined
    ? new ServerBinaryUpdateError({ step, detail })
    : new ServerBinaryUpdateError({ step, detail, cause });

/**
 * Release-asset download URL. GitHub's `releases/download/<tag>/<asset>` route
 * is stable and needs no API call or token for public repos, so an operator
 * behind a plain HTTPS egress can still update.
 */
export function serverBinaryDownloadUrl(input: {
  readonly repo: string;
  readonly version: string;
  readonly target: string;
}): string {
  const asset = serverBinaryAssetName(input.version, input.target);
  return `https://github.com/${input.repo}/releases/download/v${input.version}/${asset}`;
}

/**
 * Restart strategy for the replacement image. Under systemd we exit and let
 * `Restart=always` start the new binary, which keeps the service manager as
 * the single owner of the process; the self-exec path cannot be used there
 * because the default `KillMode=control-group` would kill the detached
 * replacement along with the exiting unit. Unsupervised, the binary spawns
 * its replacement detached and exits.
 *
 * Detection: the launcher-installed unit sets `T3_BOOT_SERVICE_UNIT`, and
 * systemd sets `INVOCATION_ID` for every unit invocation, which also covers
 * hand-written units that never went through `t3 service install`.
 */
export function resolveRestartMethod(
  env: Readonly<Record<string, string | undefined>>,
): ServerBinaryRestartMethod {
  const nonEmpty = (value: string | undefined) => value !== undefined && value.trim().length > 0;
  return nonEmpty(env[BOOT_SERVICE_UNIT_ENV]) || nonEmpty(env.INVOCATION_ID)
    ? "systemd"
    : "self-exec";
}

export interface ServerBinaryUpdatePlan {
  readonly targetVersion: string;
  readonly restart: ServerBinaryRestartMethod;
  readonly executablePath: string;
  readonly stagedPath: string;
}

/**
 * Downloads, validates and installs the replacement binary, returning the plan
 * that describes how it will be started. Nothing is restarted here: the caller
 * acknowledges the RPC first, so the client sees the handoff before the
 * connection drops.
 */
export const prepareServerBinaryUpdate = Effect.fn("cloud.binary_update.prepare")(
  function* (input: {
    readonly identity: ServerBinaryIdentity;
    readonly targetVersion: string;
    readonly reportProgress: (stage: "downloading" | "installing") => Effect.Effect<void>;
  }) {
    const fs = yield* FileSystem.FileSystem;
    const path = yield* Path.Path;
    const httpClient = yield* HttpClient.HttpClient;
    const runner = yield* ProcessRunner.ProcessRunner;
    const env = yield* HostProcessEnvironment;

    const executablePath = yield* fs
      .realPath(input.identity.executablePath)
      .pipe(Effect.orElseSucceed(() => input.identity.executablePath));
    const directory = path.dirname(executablePath);
    const url = serverBinaryDownloadUrl({
      repo: input.identity.repo,
      version: input.targetVersion,
      target: input.identity.target,
    });

    yield* input.reportProgress("downloading");
    const bytes = yield* httpClient.execute(HttpClientRequest.get(url)).pipe(
      Effect.flatMap(HttpClientResponse.filterStatusOk),
      Effect.flatMap((response) => response.arrayBuffer),
      Effect.timeout(DOWNLOAD_TIMEOUT),
      Effect.mapError((cause) =>
        fail(
          "download",
          `Could not download the ${input.targetVersion} ${input.identity.target} server binary from ${url}. Check that the release published this asset.`,
          cause,
        ),
      ),
      Effect.map((buffer) => new Uint8Array(buffer)),
    );

    yield* input.reportProgress("installing");
    // Stage inside the target directory so the activating rename stays on one
    // filesystem, and mark it non-executable-safe only after the bytes land.
    const stagedPath = path.join(directory, `.s5code-server-${input.targetVersion}.staged`);
    yield* fs.remove(stagedPath, { force: true }).pipe(Effect.ignore);
    yield* fs
      .writeFile(stagedPath, bytes)
      .pipe(
        Effect.mapError((cause) =>
          fail("write", `Could not write the staged binary to ${stagedPath}.`, cause),
        ),
      );

    return yield* Effect.gen(function* () {
      yield* fs
        .chmod(stagedPath, 0o755)
        .pipe(
          Effect.mapError((cause) =>
            fail("write", "Could not make the staged binary executable.", cause),
          ),
        );

      // Prove the download actually runs on this host and is the version we asked
      // for before it can replace a working server. A truncated download, a wrong
      // arch, or a glibc mismatch all fail here instead of at next boot.
      const reported = yield* runner
        .run({ command: stagedPath, args: ["--version"], timeout: VALIDATE_TIMEOUT })
        .pipe(
          Effect.mapError((cause) => fail("validate", "The downloaded binary did not run.", cause)),
        );
      if (reported.code !== 0) {
        return yield* fail(
          "validate",
          `The downloaded binary exited with code ${String(reported.code)} instead of reporting its version.`,
        );
      }
      if (!reported.stdout.includes(input.targetVersion)) {
        return yield* fail(
          "validate",
          `The downloaded binary reported a different version than ${input.targetVersion}.`,
        );
      }

      yield* fs
        .rename(stagedPath, executablePath)
        .pipe(
          Effect.mapError((cause) =>
            fail("activate", `Could not replace the running binary at ${executablePath}.`, cause),
          ),
        );

      return {
        targetVersion: input.targetVersion,
        restart: resolveRestartMethod(env),
        executablePath,
        stagedPath,
      } satisfies ServerBinaryUpdatePlan;
    }).pipe(
      // A failure anywhere after the write must not leave a half-installed
      // artifact next to the live binary.
      Effect.onError(() => fs.remove(stagedPath, { force: true }).pipe(Effect.ignore)),
    );
  },
);

/**
 * Hands off to the replacement image. Under systemd this just exits non-zero so
 * the unit's `Restart=always` starts the new binary; unsupervised it spawns the
 * replacement detached first.
 *
 * The caller runs this after acknowledging the RPC, so it is expected to end
 * this process.
 */
export const restartIntoServerBinary = Effect.fn("cloud.binary_update.restart")(function* (input: {
  readonly plan: ServerBinaryUpdatePlan;
  readonly argv: ReadonlyArray<string>;
  readonly exit: (code: number) => void;
}) {
  if (input.plan.restart === "self-exec") {
    const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
    // The replacement must outlive this process, so it gets a scope that is
    // never closed and is detached from this process group.
    const detachedScope = yield* Scope.make("sequential");
    // argv[1] is the /$bunfs/ entry point of the *old* image, so it is dropped;
    // everything after it is the operator's own CLI invocation.
    const args = input.argv.slice(2);
    const child = yield* spawner
      .spawn(
        ChildProcess.make(input.plan.executablePath, args, {
          detached: true,
          shell: false,
          stdin: "ignore",
          stdout: "ignore",
          stderr: "ignore",
        }),
      )
      .pipe(
        Effect.provideService(Scope.Scope, detachedScope),
        Effect.mapError((cause) =>
          fail("restart", "Could not start the replacement server binary.", cause),
        ),
      );
    yield* Effect.logInfo("Spawned the replacement server binary.", {
      pid: Number(child.pid),
      targetVersion: input.plan.targetVersion,
    });
  }

  yield* Effect.logInfo("Restarting into the updated server binary.", {
    targetVersion: input.plan.targetVersion,
    restart: input.plan.restart,
    executablePath: input.plan.executablePath,
  });
  // systemd sees a non-zero exit and applies Restart=always; unsupervised the
  // replacement is already running, so a clean exit is correct.
  yield* Effect.sync(() => input.exit(input.plan.restart === "systemd" ? 1 : 0));
});
