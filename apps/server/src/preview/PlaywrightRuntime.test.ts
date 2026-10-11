// @effect-diagnostics nodeBuiltinImport:off
import * as NodeFSP from "node:fs/promises";
import * as NodeModule from "node:module";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { expect, it } from "@effect/vitest";

import { extractPlaywrightRuntime, loadPlaywright } from "./PlaywrightRuntime.ts";

const asset = (contents: string) => ({
  size: Buffer.byteLength(contents),
  arrayBuffer: async () => new TextEncoder().encode(contents).buffer,
});

it("restores a loadable package with relative modules and package metadata", async () => {
  const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5-playwright-"));
  try {
    const entry = await extractPlaywrightRuntime(
      directory,
      new Map([
        ["./package.json", asset('{"name":"playwright-core","version":"1.60.0"}')],
        ["./index.js", asset('module.exports = require("./lib/runtime.js");')],
        ["./lib/runtime.js", asset('module.exports = require("../package.json").version;')],
      ]),
    );
    expect(NodeModule.createRequire(import.meta.url)(entry)).toBe("1.60.0");
  } finally {
    await NodeFSP.rm(directory, { recursive: true, force: true });
  }
});

it("repairs truncated cached files without rewriting intact ones", async () => {
  const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5-playwright-"));
  try {
    const files = new Map([
      ["index.js", asset("module.exports = {};")],
      ["lib/runtime.js", asset("module.exports = 42;")],
    ]);
    const entry = await extractPlaywrightRuntime(directory, files);
    const intactStat = await NodeFSP.stat(entry);
    await NodeFSP.writeFile(NodePath.join(directory, "lib/runtime.js"), "partial");
    await extractPlaywrightRuntime(directory, files);
    expect(await NodeFSP.readFile(NodePath.join(directory, "lib/runtime.js"), "utf8")).toBe(
      "module.exports = 42;",
    );
    expect((await NodeFSP.stat(entry)).mtimeMs).toBe(intactStat.mtimeMs);
  } finally {
    await NodeFSP.rm(directory, { recursive: true, force: true });
  }
});

it("keeps the installed Playwright runtime for Node and uncompiled Bun", async () => {
  const runtime = await loadPlaywright();
  expect(runtime.chromium.name()).toBe("chromium");
  expect(runtime).toBe(NodeModule.createRequire(import.meta.url)("playwright-core"));
});
