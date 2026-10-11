import * as NodeAssert from "node:assert/strict";
import * as NodeChildProcess from "node:child_process";
import * as NodeFSP from "node:fs/promises";
import * as NodeModule from "node:module";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeURL from "node:url";

const require = NodeModule.createRequire(new URL("../apps/server/package.json", import.meta.url));
const packageDirectory = NodePath.dirname(require.resolve("playwright-core/package.json"));
const executable = require("playwright-core/lib/coreBundle")
  .registry.registry.findExecutable("chromium-headless-shell")
  .executablePath();
const directory = await NodeFSP.mkdtemp(NodePath.join(NodeOS.tmpdir(), "s5-standalone-browser-"));
try {
  const archive = NodePath.join(directory, "playwright-core-runtime.tgz");
  NodeChildProcess.execFileSync("tar", [
    "-czf",
    archive,
    "--exclude=node_modules",
    "-C",
    packageDirectory,
    ".",
  ]);
  const entry = NodePath.join(directory, "browser.mjs");
  const contextsModule = NodeURL.fileURLToPath(
    new URL("../apps/server/src/preview/ServerBrowserContexts.ts", import.meta.url),
  );
  await NodeFSP.writeFile(
    entry,
    `
      import * as NodeAssert from "node:assert/strict";
      import { ServerBrowserContexts } from ${JSON.stringify(contextsModule)};
      const pool = new ServerBrowserContexts({
        profilesDir: process.env.SMOKE_PROFILES_DIR,
        executable: async () => process.env.SMOKE_CHROMIUM_EXECUTABLE,
        env: { ...process.env, T3CODE_SERVER_BROWSER_SANDBOX: "0" },
      });
      try {
        for (const isolationKey of ["agent", undefined]) {
          const context = await pool.contextFor("smoke", isolationKey);
          const page = await context.newPage();
          await page.setContent(${JSON.stringify("<button onclick=\"this.textContent='Clicked'\">Click me</button>")});
          await page.getByRole("button", { name: "Click me" }).click();
          NodeAssert.equal(await page.getByRole("button").textContent(), "Clicked");
        }
      } finally {
        await pool.close();
      }
    `,
  );
  const binary = NodePath.join(directory, "browser");
  NodeChildProcess.execFileSync(
    "bun",
    ["build", "--compile", "--splitting", entry, archive, "--outfile", binary],
    {
      cwd: directory,
      stdio: "inherit",
    },
  );
  NodeChildProcess.execFileSync(binary, [], {
    cwd: directory,
    env: {
      ...process.env,
      XDG_CACHE_HOME: NodePath.join(directory, "cache"),
      SMOKE_PROFILES_DIR: NodePath.join(directory, "profiles"),
      SMOKE_CHROMIUM_EXECUTABLE: executable,
    },
    stdio: "inherit",
    timeout: 60_000,
  });
  NodeAssert.ok(
    (await NodeFSP.readdir(NodePath.join(directory, "cache", "s5code", "browser"))).some((name) =>
      name.startsWith("playwright-core-runtime-"),
    ),
    "Compiled browser did not extract its embedded Playwright runtime",
  );
  console.log(
    "Standalone browser opened isolated and persistent previews and clicked page controls.",
  );
} finally {
  await NodeFSP.rm(directory, { recursive: true, force: true });
}
