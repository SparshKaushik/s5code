import * as NodeFs from "node:fs";
import * as NodePath from "node:path";
import { fileURLToPath } from "node:url";

import { expect, it } from "@effect/vitest";

const SOURCE_ROOT = NodePath.resolve(fileURLToPath(new URL("..", import.meta.url)));

const ALLOWED_STATIC_NODE_SQLITE_FILES = new Set([
  "persistence/NodeSqliteClient.ts",
  "usage/usageCursorNodeSqlite.ts",
]);

const STATIC_IMPORT_RE =
  /(?:import\s+(?:[\s\S]*?\s+from\s+)?|export\s+[\s\S]*?\s+from\s+)["']node:sqlite["']|require\s*\(\s*["']node:sqlite["']\s*\)/;

function listTypeScriptFiles(directory: string): ReadonlyArray<string> {
  const entries = NodeFs.readdirSync(directory, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = NodePath.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "dist") continue;
      files.push(...listTypeScriptFiles(fullPath));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".ts") && !entry.name.endsWith(".test.ts")) {
      files.push(fullPath);
    }
  }
  return files;
}

function relativeSourcePath(absolutePath: string): string {
  return NodePath.relative(SOURCE_ROOT, absolutePath).split(NodePath.sep).join("/");
}

it("keeps node:sqlite off the compiled binary startup graph", () => {
  const files = listTypeScriptFiles(SOURCE_ROOT);
  const staticImporters: string[] = [];

  for (const absolutePath of files) {
    const relativePath = relativeSourcePath(absolutePath);
    const source = NodeFs.readFileSync(absolutePath, "utf8");
    if (!STATIC_IMPORT_RE.test(source)) continue;
    staticImporters.push(relativePath);
  }

  expect(staticImporters.sort()).toEqual([...ALLOWED_STATIC_NODE_SQLITE_FILES].sort());

  for (const allowed of ALLOWED_STATIC_NODE_SQLITE_FILES) {
    const basename = NodePath.posix.basename(allowed, ".ts");
    const staticReference = new RegExp(String.raw`from\s+["'][^"']*${basename}(?:\.ts)?["']`);
    const dynamicReference = new RegExp(
      String.raw`import\(\s*["'][^"']*${basename}(?:\.ts)?["']\s*\)`,
    );

    let seenDynamic = false;
    for (const absolutePath of files) {
      const relativePath = relativeSourcePath(absolutePath);
      if (relativePath === allowed) continue;
      const source = NodeFs.readFileSync(absolutePath, "utf8");
      expect(
        staticReference.test(source),
        `${allowed} must not be statically imported from ${relativePath}`,
      ).toBe(false);
      if (dynamicReference.test(source)) seenDynamic = true;
    }
    expect(seenDynamic, `${allowed} must be reached via dynamic import()`).toBe(true);
  }
});
