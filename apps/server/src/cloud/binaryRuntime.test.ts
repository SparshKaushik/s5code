import { expect, it } from "@effect/vitest";
import * as Option from "effect/Option";

import {
  isStandaloneBunExecutable,
  resolveServerBinaryIdentity,
  serverBinaryAssetName,
} from "./binaryRuntime.ts";

const buildValues = {
  target: "linux-x64" as string | undefined,
  repo: "SparshKaushik/s5code" as string | undefined,
};

const binaryArgv = ["/usr/local/bin/s5code-server", "/$bunfs/root/bin", "serve"];

it("detects a compiled binary only from the /$bunfs/ entry point", () => {
  expect(isStandaloneBunExecutable(binaryArgv)).toBe(true);
  // `npx t3` and dev runs both have a real path as argv[1].
  expect(isStandaloneBunExecutable(["/usr/bin/node", "/opt/t3/dist/bin.mjs"])).toBe(false);
  expect(isStandaloneBunExecutable(["/usr/bin/node"])).toBe(false);
});

it("names release assets the way the build script writes them", () => {
  expect(serverBinaryAssetName("1.2.3", "linux-arm64")).toBe("s5code-server-1.2.3-linux-arm64");
});

it("resolves identity for a well-formed release binary", () => {
  const identity = resolveServerBinaryIdentity({
    argv: binaryArgv,
    executablePath: "/usr/local/bin/s5code-server",
    ...buildValues,
  });
  expect(Option.isSome(identity)).toBe(true);
  if (Option.isSome(identity)) {
    expect(identity.value).toEqual({
      target: "linux-x64",
      repo: "SparshKaushik/s5code",
      executablePath: "/usr/local/bin/s5code-server",
    });
  }
});

it("refuses identity when the process is not a compiled binary", () => {
  expect(
    Option.isNone(
      resolveServerBinaryIdentity({
        argv: ["/usr/bin/node", "/opt/t3/dist/bin.mjs"],
        executablePath: "/usr/bin/node",
        ...buildValues,
      }),
    ),
  ).toBe(true);
});

it.each([
  ["missing target", { target: undefined, repo: "owner/repo" }],
  ["missing repo", { target: "linux-x64", repo: undefined }],
  ["blank target", { ...buildValues, target: "  " }],
  ["unsupported target", { ...buildValues, target: "linux-riscv64" }],
  ["malformed repo", { ...buildValues, repo: "not-a-repo" }],
  ["repo with traversal", { ...buildValues, repo: "owner/../repo" }],
])("refuses identity for a half-configured build: %s", (_label, values) => {
  expect(
    Option.isNone(
      resolveServerBinaryIdentity({
        argv: binaryArgv,
        executablePath: "/usr/local/bin/s5code-server",
        ...values,
      }),
    ),
  ).toBe(true);
});
