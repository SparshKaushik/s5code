// @effect-diagnostics nodeBuiltinImport:off
import * as NodeFSP from "node:fs/promises";
import * as NodeModule from "node:module";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeVM from "node:vm";
import { expect, it } from "@effect/vitest";

import { patchStandaloneCursorSdk } from "./standalone-cursor-sdk.ts";

it("resolves the pinned SDK's native helpers from the standalone cache", async () => {
  const directory = await NodeFSP.mkdtemp(
    NodePath.join(NodeOS.tmpdir(), "s5code-cursor-resolver-"),
  );
  try {
    const require = NodeModule.createRequire(
      new URL("../../apps/server/package.json", import.meta.url),
    );
    const source = patchStandaloneCursorSdk(
      await NodeFSP.readFile(require.resolve("@cursor/sdk/bundled"), "utf8"),
    );
    // Run the actual SDK resolver, with the filesystem primitives it imports.
    const resolverSource = source.slice(
      source.indexOf("function RZ1($){"),
      source.indexOf("function CZ1($){"),
    );
    const env: NodeJS.ProcessEnv = {
      T3CODE_CURSOR_SDK_PLATFORM_DIR: NodePath.join(directory, "cache"),
    };
    const resolve = NodeVM.runInNewContext(`(${resolverSource})`, {
      process: {
        platform: "linux",
        arch: "x64",
        argv: ["server", NodePath.join(directory, "bin/server")],
        execPath: NodePath.join(directory, "bin/server"),
        env,
      },
      TZ1: NodePath.join,
      V_0: NodePath.resolve,
      Pi0: NodePath.dirname,
      vf1: NodePath.parse,
      LS8: () => undefined,
      BS8: () => false,
    }) as (input: {
      relativePath: string;
      excludedWorkspaceDirs: string[];
      accept: (path: string) => boolean;
    }) => string | undefined;
    const paths = new Set<string>();
    for (const name of ["bin/rg", "bin/cursorsandbox", "vendor"]) {
      const cached = NodePath.join(env.T3CODE_CURSOR_SDK_PLATFORM_DIR!, name);
      paths.add(cached);
      expect(
        resolve({
          relativePath: name,
          excludedWorkspaceDirs: [],
          accept: (path) => paths.has(path),
        }),
      ).toBe(cached);
    }

    const installed = NodePath.join(directory, "node_modules/@cursor/sdk-linux-x64/bin/rg");
    paths.add(installed);
    // An absent cached helper preserves the SDK's disk-backed lookup.
    env.T3CODE_CURSOR_SDK_PLATFORM_DIR = NodePath.join(directory, "missing");
    expect(
      resolve({
        relativePath: "bin/rg",
        excludedWorkspaceDirs: [],
        accept: (path) => paths.has(path),
      }),
    ).toBe(installed);
    delete env.T3CODE_CURSOR_SDK_PLATFORM_DIR;
    expect(
      resolve({
        relativePath: "bin/rg",
        excludedWorkspaceDirs: [],
        accept: (path) => paths.has(path),
      }),
    ).toBe(installed);
  } finally {
    await NodeFSP.rm(directory, { recursive: true, force: true });
  }
});

it("fails packaging when an SDK update changes native-helper discovery", () => {
  expect(() => patchStandaloneCursorSdk("export class Cursor {}")).toThrow(
    "native-helper resolver changed",
  );
});
