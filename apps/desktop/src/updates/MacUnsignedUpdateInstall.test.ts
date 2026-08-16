import { assert, describe, it } from "@effect/vitest";

import {
  findMacAppBundleName,
  isSquirrelCompatibleMacSignature,
  macBundleReplaceShellScript,
  resolveMacAppBundlePath,
} from "./MacUnsignedUpdateInstall.ts";

describe("isSquirrelCompatibleMacSignature", () => {
  it("rejects ad-hoc signatures that Squirrel.Mac cannot replace", () => {
    assert.isFalse(
      isSquirrelCompatibleMacSignature(
        [
          "Executable=/Applications/S5 Code (Alpha).app/Contents/MacOS/S5 Code (Alpha)",
          "Identifier=club.touchtech.s5code",
          "Format=app bundle with Mach-O thin (arm64)",
          "Signature=adhoc",
          "TeamIdentifier=not set",
        ].join("\n"),
      ),
    );
  });

  it("rejects linker-signed unsigned Electron binaries", () => {
    assert.isFalse(
      isSquirrelCompatibleMacSignature(
        [
          "Identifier=Electron",
          "Signature=adhoc",
          "Info.plist=not bound",
          "TeamIdentifier=not set",
        ].join("\n"),
      ),
    );
  });

  it("accepts Developer ID signatures with a team identifier", () => {
    assert.isTrue(
      isSquirrelCompatibleMacSignature(
        [
          "Identifier=club.touchtech.s5code",
          "Authority=Developer ID Application: Touchtech Club (ABCDE12345)",
          "Authority=Developer ID Certification Authority",
          "Authority=Apple Root CA",
          "TeamIdentifier=ABCDE12345",
        ].join("\n"),
      ),
    );
  });

  it("rejects empty codesign output", () => {
    assert.isFalse(isSquirrelCompatibleMacSignature(""));
  });
});

describe("findMacAppBundleName", () => {
  it("returns the only .app entry", () => {
    assert.equal(
      findMacAppBundleName(["Contents", "S5 Code (Alpha).app", "README.txt"]),
      "S5 Code (Alpha).app",
    );
  });

  it("returns null when the zip has no app bundle", () => {
    assert.isNull(findMacAppBundleName(["update.zip", "latest-mac.yml"]));
  });

  it("returns null when the zip has multiple app bundles", () => {
    assert.isNull(findMacAppBundleName(["S5 Code.app", "Unexpected.app"]));
  });
});

describe("resolveMacAppBundlePath", () => {
  it("walks up from app.asar to the .app bundle", () => {
    assert.equal(
      resolveMacAppBundlePath("/Applications/S5 Code (Alpha).app/Contents/Resources/app.asar"),
      "/Applications/S5 Code (Alpha).app",
    );
  });

  it("walks up from the MacOS executable to the .app bundle", () => {
    assert.equal(
      resolveMacAppBundlePath("/Applications/S5 Code (Alpha).app/Contents/MacOS/S5 Code (Alpha)"),
      "/Applications/S5 Code (Alpha).app",
    );
  });

  it("returns the path when it is already an app bundle", () => {
    assert.equal(
      resolveMacAppBundlePath("/Applications/S5 Code (Alpha).app"),
      "/Applications/S5 Code (Alpha).app",
    );
  });

  it("returns null when no app bundle is on the path", () => {
    assert.isNull(resolveMacAppBundlePath("/repo/apps/desktop"));
  });
});

describe("macBundleReplaceShellScript", () => {
  it("waits for the captured pid then replaces and relaunches the app", () => {
    const script = macBundleReplaceShellScript({
      pid: 4242,
      sourceAppPath: "/tmp/update's/S5 Code (Alpha).app",
      destAppPath: "/Applications/S5 Code (Alpha).app",
      cleanupPath: "/tmp/update's",
    });

    assert.include(script, "kill -0 4242");
    assert.include(script, "mv '/Applications/S5 Code (Alpha).app' \"$backup\"");
    assert.include(script, "ditto '/tmp/update'\\''s/S5 Code (Alpha).app'");
    assert.include(script, "mv \"$backup\" '/Applications/S5 Code (Alpha).app'");
    assert.include(script, "rm -rf \"$backup\" '/tmp/update'\\''s'");
    assert.include(script, "open '/Applications/S5 Code (Alpha).app'");
  });
});
