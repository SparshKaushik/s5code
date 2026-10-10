// @effect-diagnostics nodeBuiltinImport:off
import * as NodeFSP from "node:fs/promises";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import { expect, it } from "@effect/vitest";

import { extractCursorNativeAssets } from "./CursorNativeAssets.ts";

const asset = (contents: string) => ({
  size: new TextEncoder().encode(contents).byteLength,
  arrayBuffer: () => Promise.resolve(new TextEncoder().encode(contents).buffer),
});

it("extracts runnable helpers and disk-backed tree-sitter modules", async () => {
  const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5code-cursor-assets-"));
  try {
    await extractCursorNativeAssets(
      directory,
      new Map([
        ["bin/rg", asset("ripgrep")],
        ["bin/cursorsandbox", asset("sandbox")],
        ["vendor/tree-sitter/index.js", asset("module.exports = {}")],
        ["vendor/tree-sitter/binding.node", asset("native-binding")],
      ]),
    );
    expect(await NodeFSP.readFile(NodePath.join(directory, "bin/rg"), "utf8")).toBe("ripgrep");
    for (const name of ["bin/rg", "bin/cursorsandbox", "vendor/tree-sitter/binding.node"]) {
      expect((await NodeFSP.stat(NodePath.join(directory, name))).mode & 0o111).not.toBe(0);
    }
    expect((await NodeFSP.readdir(NodePath.join(directory, "vendor/tree-sitter"))).sort()).toEqual([
      "binding.node",
      "index.js",
    ]);
  } finally {
    await NodeFSP.rm(directory, { recursive: true, force: true });
  }
});

it("repairs truncated or nonexecutable helpers and reuses intact files", async () => {
  const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5code-cursor-assets-"));
  try {
    const files = new Map([
      ["bin/rg", asset("ripgrep")],
      ["bin/cursorsandbox", asset("sandbox")],
      ["vendor/tree-sitter/index.js", asset("module.exports = {}")],
    ]);
    await extractCursorNativeAssets(directory, files);
    const intactPath = NodePath.join(directory, "vendor/tree-sitter/index.js");
    const intactStat = await NodeFSP.stat(intactPath);
    await NodeFSP.writeFile(NodePath.join(directory, "bin/rg"), "truncated");
    await NodeFSP.chmod(NodePath.join(directory, "bin/cursorsandbox"), 0o644);
    await extractCursorNativeAssets(directory, files);
    expect(await NodeFSP.readFile(NodePath.join(directory, "bin/rg"), "utf8")).toBe("ripgrep");
    expect(
      (await NodeFSP.stat(NodePath.join(directory, "bin/cursorsandbox"))).mode & 0o111,
    ).not.toBe(0);
    expect((await NodeFSP.stat(intactPath)).mtimeMs).toBe(intactStat.mtimeMs);
  } finally {
    await NodeFSP.rm(directory, { recursive: true, force: true });
  }
});
