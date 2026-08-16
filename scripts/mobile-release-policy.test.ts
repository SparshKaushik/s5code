// @effect-diagnostics nodeBuiltinImport:off - This static policy test reads repository source files as fixtures.
import * as NodeFS from "node:fs";
import * as NodePath from "node:path";
import { describe, expect, it } from "@effect/vitest";

const repoRoot = NodePath.resolve(import.meta.dirname, "..");
const readRepoFile = (relativePath: string) =>
  NodeFS.readFileSync(NodePath.join(repoRoot, relativePath), "utf8");

describe("mobile release policy", () => {
  it("keeps production versions CI-owned and uses appVersion as the runtime boundary", () => {
    const appConfig = readRepoFile("apps/mobile/app.config.ts");

    expect(appConfig).toContain("...(releaseVersion ? { version: releaseVersion } : {})");
    expect(appConfig).toContain('APP_VARIANT === "production" ? "appVersion" : "fingerprint"');
    expect(appConfig).not.toMatch(/version:\s*releaseVersion\s*\?\?\s*"(?!0\.0\.0)[^"]+"/);
  });

  it("routes unified releases through fingerprint-gated mobile reconciliation", () => {
    const releaseWorkflow = readRepoFile(".github/workflows/release.yml");
    const mobileWorkflow = readRepoFile(".github/workflows/mobile-eas-production.yml");

    expect(releaseWorkflow).toContain("uses: ./.github/workflows/mobile-eas-production.yml");
    expect(releaseWorkflow).toContain("release_version: ${{ needs.preflight.outputs.version }}");
    // Android builds run locally on the runner (eas build --local) instead of
    // EAS's cloud build queue; release.yml attaches the fingerprint.txt asset
    // that mobile-eas-production.yml's next run compares against.
    expect(releaseWorkflow).toContain("release-assets/fingerprint.txt");

    expect(mobileWorkflow).toContain("Compare fingerprint with latest release");
    expect(mobileWorkflow).toContain("eas fingerprint:generate");
    expect(mobileWorkflow).toContain("eas build \\\n            --local");
    expect(mobileWorkflow).toContain('echo "mode=update"');
    expect(mobileWorkflow).toContain('echo "mode=build"');
    // OTA-only releases attach no fingerprint.txt, so the decision walks
    // releases newest-first (.[].tagName) and uses the most recent one that
    // recorded a fingerprint, rather than rebuilding after every OTA.
    expect(mobileWorkflow).toContain("sort_by(.createdAt) | reverse | .[].tagName");
    expect(mobileWorkflow).toContain("while IFS= read -r tag");
    // appVersion participates in Expo's native fingerprint. Compare with the
    // previous binary's version, but record a new build's fingerprint using
    // the release version that is actually injected into that binary.
    expect(mobileWorkflow).toContain(
      'comparison_fingerprint="$(fingerprint_for_version "$previous_version")"',
    );
    expect(mobileWorkflow).toContain(
      'build_fingerprint="$(fingerprint_for_version "$RELEASE_VERSION")"',
    );
    expect(mobileWorkflow).toContain('MOBILE_VERSION="$mobile_version" eas fingerprint:generate');
    expect(mobileWorkflow).toContain(
      "MOBILE_VERSION: ${{ steps.release-decision.outputs.mobile_version }}",
    );
    expect(mobileWorkflow).toContain("eas env:set production");
    expect(mobileWorkflow).toContain("--name MOBILE_VERSION");
    expect(mobileWorkflow).not.toContain(".build.production.env.MOBILE_VERSION = $version");
  });
});
