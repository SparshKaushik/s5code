import * as NodeChildProcess from "node:child_process";
import * as NodeFS from "node:fs";
import * as NodePath from "node:path";
import { describe, expect, it } from "vite-plus/test";

describe("OpenCode2 bundled integration", () => {
  it("verifies built server bundle can load and initialize embedded OpenCode 2 host", async () => {
    const binDistPath = NodePath.resolve(import.meta.dirname ?? "", "../dist/bin.mjs");
    if (!NodeFS.existsSync(binDistPath)) {
      // Skip if build artifact has not been produced yet
      return;
    }

    const testScript = `
      await import(${JSON.stringify(binDistPath)});
      const fs = await import("node:fs");
      const binMjs = fs.readFileSync(${JSON.stringify(binDistPath)}, "utf8");
      const match = binMjs.match(/await import\\(["']\\.\\/(dist-[^"']+)["']\\)/);
      if (!match) throw new Error("Could not find OpenCode chunk in bin.mjs");
      const chunk = match[1];
      const mod = await import("./" + chunk);
      const tempDir = fs.mkdtempSync(nodePath.join(os.tmpdir(), "opencode2-test-"));
      const host = await mod.OpenCode.create({
        database: { path: nodePath.join(tempDir, "sessions.db") },
        instances: { default: { directory: tempDir } }
      });
      if (!host || typeof host.sessions?.create !== "function") {
        throw new Error("Invalid host returned");
      }
      await host.close();
      console.log("OPENCODE2_HOST_INITIALIZATION_SUCCESS");
    `;

    const runner = `
      import * as nodePath from "node:path";
      import * as os from "node:os";
      ${testScript}
    `;

    const result = NodeChildProcess.spawnSync(
      process.execPath,
      ["--input-type=module", "-e", runner],
      {
        cwd: NodePath.dirname(binDistPath),
        encoding: "utf8",
        timeout: 30_000,
      },
    );

    if (result.status !== 0) {
      console.error("Runner stdout:", result.stdout);
      console.error("Runner stderr:", result.stderr);
    }
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("OPENCODE2_HOST_INITIALIZATION_SUCCESS");
  });
});
