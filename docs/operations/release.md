# Release Checklist

> For maintainers. Using T3 Code? See [docs/user](../user/).

This document covers the unified release workflow for stable and nightly desktop, server, web, and mobile releases.

## What the workflow does

- Workflow: `.github/workflows/release.yml`
- Triggers:
- Workflow: `.github/workflows/release.yml`
- Triggers:
  - manual `workflow_dispatch` with `channel=stable`, the normal way to ship stable. Stable
    and nightly dispatches must select `main`; preview may select any branch. The channel defaults
    to preview so an omitted selection cannot publish a stable release.
  - push tag matching `v*.*.*` for a stable release of an explicit commit
  - scheduled nightly check every 30 minutes
  - manual `workflow_dispatch` with `channel=nightly`
  - manual `workflow_dispatch` with `channel=preview`, the maintainers' test train. It exercises the whole release flow (build, package, smoke, publish) for a commit that end users must never receive, which is how an unmerged branch or a risky change gets a real release run before it lands. It builds the triggering commit with nightly's versioning under the `preview` prerelease identifier and publishes a GitHub prerelease. Preview is not on the schedule, its desktop builds carry no update feed, and no updater manifest (`latest*.yml`, `nightly*.yml`, blockmaps) is attached, so a stable or nightly install cannot be offered one. The release itself is named as a maintainer test build and its body is a warning rather than generated notes: a changelog of unmerged branch history is not a changelog, and nightly and stable notes are unaffected because each series resolves its previous tag within its own channel.
- A manual stable release builds the commit of the latest published nightly, not `main` HEAD.
  Nightly is the release candidate: verify the nightly, then promote it. Merges to `main` keep
  landing while you verify and never leak into the stable build.
  - The version defaults to the one the nightly previewed (`0.0.39-nightly.*` ships as `0.0.39`).
    Pass the `version` input to override it, for example for a minor bump.
  - The stable tag is created on the nightly's commit when the GitHub Release is published.
  - Pushing a `vX.Y.Z` tag by hand still works and builds exactly the tagged commit. Use it when
    the commit to ship is not the latest nightly, such as a cherry-picked fix on a release branch.
- Runs lint, typecheck, and tests alongside artifact builds. Publishing waits for every check.
- Emits the fork's self-hosted T3 Connect relay URL and Clerk client configuration (static values
  in the `relay_public_config` job) before packaging clients.
- Builds the platform-independent JS (server bundle, web client, Electron main) once in the `build_bundle` job and hands it to every platform job as the `js-bundle` artifact; the platform jobs only package it, so no runner rebuilds it.
- Builds six desktop artifacts in parallel for both channels, each as its own job (`desktop_<platform>_<arch>`, one call of `release-desktop.yml`) on hardware of its own architecture, gated only on the bundle (the Windows jobs also wait for the same-arch Linux job, whose CLI archive they embed as the WSL runtime):
  - macOS `arm64` DMG
  - macOS `x64` DMG
  - Linux `x64` and `arm64` AppImage
  - Windows `x64` and `arm64` NSIS installer
- Builds a self-contained CLI archive per platform (`t3-<version>-<platform>-<arch>.tar.gz`, `.zip` on Windows) in the same job as that target's desktop artifact and attaches them to the GitHub Release with a `SHA256SUMS` file, on every channel, for five targets: macOS arm64, Linux x64 and arm64, Windows x64 and arm64. Every archive is built and smoke-tested on hardware of its own architecture. There is no macOS x64 archive: Node single-executables are unsupported on x64 macOS (the SEA docs list macOS as arm64 only) and the binary segfaults on start; the x64 desktop app is Electron and unaffected.
  - The archive holds the server as a Node single-executable (`scripts/build-cli-archive.ts`), so unpacking it needs neither Node, npm, nor a compiler. It is the form T3 Code manages a runtime in: the desktop's SSH environments, the boot service, `t3 update`, and the install scripts all download and verify this archive against `SHA256SUMS`. The `curl | sh` installers are `scripts/install.sh` and `scripts/install.ps1`.
  - The executable is built with a Node that supports `--build-sea` (`VP_NODE_VERSION=26.8.2`, kept in step with `SEA_NODE_VERSION` in `apps/server/vite.config.ts`), while the repo stays on `engines.node`.
  - Each archive is extracted and executed on its build runner (`scripts/smoke-cli-archive.ts`) before it is uploaded.
- Builds self-updating Linux server binaries (`s5code-server-<version>-linux-<arch>`) on matching-arch runners; the binary self-update path in `apps/server/src/cloud/selfUpdate.ts` downloads them from the release.
- Reconciles the Android mobile release through EAS:
  - if the current native fingerprint matches the fingerprint recorded on the previous GitHub Release, publishes an OTA update reusing that release's mobile version
  - otherwise, injects the unified release version, builds a new Android APK locally on the runner (`eas build --local`, signed with EAS-managed remote credentials but not run on EAS's cloud queue), and attaches it as `s5code-<version>.apk` alongside a `fingerprint.txt` recording the native fingerprint it was built from
- Publishes one GitHub Release with all produced files. OTA-only releases intentionally reuse the previous mobile binary and therefore have no new APK asset.

  - Stable tags with a suffix after `X.Y.Z` (for example `1.2.3-alpha.1`) are published as GitHub prereleases.
  - Only plain stable `X.Y.Z` releases are marked as the repository's latest release.
  - Nightly runs are always GitHub prereleases and never marked latest.
  - Automatically generated release notes are pinned to the previous tag in the same channel, so stable compares to the previous stable tag and nightly compares to the previous nightly tag.

- Includes Electron auto-update metadata (for example `latest*.yml`, `nightly*.yml`, and `*.blockmap`) in release assets.
- Signing is optional and auto-detected per platform from secrets: Apple secrets sign the DMGs. Android APKs are always signed, using the credentials EAS manages remotely for the "production" build profile (downloaded to the runner over `EXPO_TOKEN` at build time). Mobile reconciliation requires EAS configuration and fails rather than silently omitting a requested OTA or build.

## Mobile release invariant

Mobile app versions are CI-owned release metadata. Never edit or add a mobile release version in a source commit. `apps/mobile/app.config.ts` omits the version field unless CI supplies `MOBILE_VERSION`; release build and update commands always receive that value from CI.

The production runtime policy is `appVersion`; Expo Fingerprint is the release decision input, not the runtime version. Because `MOBILE_VERSION` supplies that app version and therefore participates in the native fingerprint, CI generates the comparison fingerprint with the previous binary's version. A match publishes JavaScript and assets to that exact runtime version. A mismatch generates a second fingerprint with the unified release version, builds that new binary, and records that second hash in `fingerprint.txt`. This ensures the recorded hash always describes the APK that was actually built rather than whichever version happened to be present in the EAS environment before reconciliation. Before publishing or building, CI synchronizes the selected version to the EAS production environment so local and remote app-config evaluation agree; source files remain unchanged. `eas.json`'s `autoIncrement` for `versionCode` is unaffected by local builds: with `appVersionSource: "remote"`, the version code is a counter EAS's servers track and bump via API on every build, independent of `eas build:list` history.

### Why builds run locally instead of on EAS's cloud queue

Android builds run with `eas build --local`, which executes the actual Gradle build on the GitHub Actions runner instead of queueing it on EAS's cloud build infrastructure. This avoids EAS's shared build queue wait times. Credentials are unaffected: `eas.json`'s `credentialsSource` for the `production` profile still defaults to `remote`, so the signing keystore is downloaded from EAS (authenticated via `EXPO_TOKEN`) at build time, the same as a cloud build. No keystore or signing secret lives in this repository.

The tradeoff: `eas build --local` builds are never recorded in EAS's build history (`eas build:list` only sees cloud builds), which is what the OTA-vs-new-build fingerprint comparison used previously. This repo now tracks that itself: every native build attaches a `fingerprint.txt` file next to the APK on its GitHub Release, and the next release compares against the most recent release that carries a `fingerprint.txt` (skipping past OTA-only releases, which attach none) instead of querying EAS. If no release yet carries a `fingerprint.txt` — a first release, or every prior release predates this mechanism — the run falls back to building rather than guessing.

PR preview builds (`.github/workflows/mobile-eas-preview.yml`) only publish OTA updates for this same reason: a fresh per-PR dev-client APK can no longer be tracked against EAS's build history to decide when one is actually needed, and building one on every push is wasteful. When a PR's native fingerprint changes, reviewers rebuild their local dev client with `pnpm --filter @t3tools/mobile eas:android:preview:dev` (still an EAS cloud build; this is a manual, infrequent developer action, not CI).

Required repository configuration:

- Secret `EXPO_TOKEN`
- Variables `EAS_PROJECT_ID` and `EAS_OWNER`

The EAS `production` environment must contain the public mobile build configuration used by the app. Manual runs of `.github/workflows/mobile-eas-production.yml` are recovery controls and require an explicit mobile version.

## Pull request macOS previews

Labeling a PR `preview:mac` publishes an Apple Silicon DMG with T3 Connect enabled
to the rolling `desktop-preview` prerelease, and works for fork PRs. The label is a one-shot request
for the commit it is applied to: the trusted workflow removes it once the build is in hand, and later
pushes do not build until a maintainer applies it again. Builds are signed and notarized when the
Apple secrets below are configured, ad hoc otherwise. Vouching a contributor lets their labeled
commits be packaged; it is not a standing grant. The build is
split so the packaging secrets never share a job with PR code:

- `.github/workflows/desktop-macos-preview.yml` runs on `pull_request` with no secrets and builds
  only the JS bundle from the PR (the same `js-bundle` artifact `release.yml` produces).
- `.github/workflows/desktop-macos-preview-publish.yml` runs on `workflow_run` from `main`. It
  refuses unless the PR is open, still labeled, its head is the built commit, and the author is a
  bot, a collaborator, or listed in `.github/VOUCHED.td` (read from the default branch, so a PR cannot vouch
  for itself). It then packages the bundle through `release-desktop.yml` checked out at
  `main`, so packaging, native helpers, and the Electron/desktop dependencies come from `main`, not
  the PR. Only the version and the public T3 Connect identifiers in `.env.example` are read from the
  PR commit, as data, so a signed app's passkey entitlement matches the bundle. A PR that changes
  packaging must use the `channel=preview` release train above instead.

Before handing the bundle to the packaging runner, the trusted workflow validates its ZIP entries
and accepts only regular files under `server/dist` and `desktop/dist-electron`, plus the directory
entries that lead to those roots. The artifact cannot
overwrite packaging code or installed dependencies. The bundle is copied into the app, never executed,
on the packaging runner. The
`pull_request_target` cleanup job in the publish workflow removes the download when the PR closes, or
when the label is removed by hand before a build consumed it, and never checks out PR code.

## Mobile signing

Android APK signing uses the credentials EAS manages for the configured project (see
`eas.json`, `credentialsSource` defaults to `remote`). No Android secrets are stored in this
repository.

## T3 Connect relay deployment

The fork's relay is a self-hosted Cloudflare Worker backed by Neon Postgres, deployed by
`.github/workflows/deploy-relay.yml` on pushes to `main` (it calls `infra/relay/scripts/deploy.ts`
through `vp run --filter t3code-relay deploy`). Stable and nightly client builds must point at the
same relay so users see the same linked environments when switching release channels.

`release.yml` does not read relay config back from the deploy: the `relay_public_config` job emits
the same public identifiers that `.env.example` carries (`T3CODE_CLERK_PUBLISHABLE_KEY`,
`T3CODE_JWT_TEMPLATE`-style template name, `T3CODE_CLERK_CLI_OAUTH_CLIENT_ID`, `T3CODE_RELAY_URL`).
Update both places together if the relay or Clerk instance moves.

The deploy workflow reads Cloudflare/Neon/Clerk credentials from the `production` GitHub Actions
environment. See `.github/workflows/deploy-relay.yml` and `infra/relay/scripts/deploy.ts` for the
exact variable and secret names.

## Hosted web app deployment

The hosted web app is deployed separately from releases: `.github/workflows/deploy-web.yml` builds
`apps/web` against the self-hosted relay and Clerk config and uploads it to Cloudflare Pages on
`workflow_dispatch`. The Pages site serves `/pair` and the `/connect` + `/connect/callback` CLI
OAuth routes, so headless servers can complete `t3 connect link` without a local browser. See the
workflow file's header comment for the one-time Cloudflare Pages and Clerk setup.

## Nightly builds

- Workflow: `.github/workflows/release.yml`
- Triggers:
  - manual `workflow_dispatch` with `channel=nightly`
- Runs the same desktop quality gates and artifact matrix as the tagged release flow.
- Publishes a GitHub prerelease only:
  - current tag format: `vX.Y.Z-nightly.YYYYMMDD.<run_number>`
  - `nightly-v...` is accepted only as a legacy previous-nightly tag
  - release name includes the short commit SHA
  - `make_latest` is always `false`
- Uses the next stable patch version as the nightly base. For example, `0.0.17` produces nightlies on `0.0.18-nightly.*`.
- Publishes Electron auto-update metadata to the dedicated `nightly` updater channel, so desktop users can opt into that track independently from stable.
- Attaches the same `t3-<version>-<platform>` CLI archives and updater manifests as a stable release.
- Does not commit version bumps back to `main`.

## Server self-update release invariant

Connected servers update to the client's exact version. Every released client version must
therefore carry matching server runtime assets on the GitHub Release before users can receive
that client.

The fork ships two server runtime forms on every release:

- `t3-<version>-<platform>.tar.gz`/`.zip` CLI archives (the Node single-executables the install
  scripts, `t3 update`, the boot service, and the desktop's SSH/WSL environments download and
  verify against `SHA256SUMS`).
- `s5code-server-<version>-linux-<arch>` Bun-compiled binaries for the fork's binary self-update
  path in `apps/server/src/cloud/selfUpdate.ts`.

The `release` job waits on every build job (desktop, server, mobile) before publishing, so no
release can exist with a client version whose server assets are missing. Preserve that dependency
when changing the release graph.

For a release smoke test, download the release's `SHA256SUMS` and the matching `t3-<version>`
archive, verify the checksum, then connect the new client to a server on the previous version and
verify that the update action reconnects to the matching server. When the release adds database
migrations, verify that the remote update applies them and reconnects. A failed trial must restore
the database snapshot and restart the previous server.

## Desktop auto-update notes

- Updater runtime: `apps/desktop/src/updates/DesktopUpdates.ts`.
- `electron-updater` adapter: `apps/desktop/src/electron/ElectronUpdater.ts`.
- `apps/desktop/src/main.ts` only wires the updater layers into the desktop runtime.
- Update UX:
  - Background checks run on startup delay + interval.
  - No automatic download or install.
  - The desktop UI shows a rocket update button when an update is available; click once to download, click again after download to restart/install.
- Provider: GitHub Releases (`provider: github`) configured at build time.
- Repository slug source:
  - `T3CODE_DESKTOP_UPDATE_REPOSITORY` (format `owner/repo`), if set.
  - otherwise `GITHUB_REPOSITORY` from GitHub Actions.
- Required release assets for updater:
  - platform installers (`.exe`, `.dmg`, `.AppImage`, plus macOS `.zip` for Squirrel.Mac update payloads)
  - channel metadata: `latest*.yml` for stable releases, `nightly*.yml` for nightly releases
  - `*.blockmap` files (used for differential downloads)
- macOS metadata note:
  - `electron-updater` reads `latest-mac.yml` on stable and `nightly-mac.yml` on nightly, for both Intel and Apple Silicon.
  - The workflow merges the per-arch mac manifests into one channel-specific mac manifest before publishing the GitHub Release.

### Windows payload topology and update validation

Windows packages the bundled server and only its runtime-external/native
dependency closure in `resources/server.asar`. Native modules and helper
executables declared as unpacked by that archive must be present at the matching
paths below `resources/server.asar.unpacked`. The Windows-native backend reads
the archive in place through Electron. Packaged Windows builds also ship
`resources/wsl-runtime.tar.gz` plus its SHA-256 sidecar: the Linux CLI archive
(`t3-<version>-linux-<arch>.tar.gz`, the same arch as the Windows host) built
by the Linux desktop job and handed to the Windows desktop build as
`--wsl-runtime`, copied in verbatim so WSL runs the exact bytes a Linux user
downloads. WSL verifies and extracts that archive
into `~/.t3/wsl-runtime/sha256-<archive-digest>` inside the selected distro,
then reuses it for later launches of the same update.

The artifact builder rejects a Windows package when any of these invariants
break:

- `resources/server.asar` is absent or does not contain the server entry.
- Any file marked unpacked in the ASAR header is absent from
  `resources/server.asar.unpacked`.
- On same-architecture Windows builds, the packaged primary cannot load the fff
  native library from inside `server.asar` through its `.unpacked` sibling.
- The isolated, extracted sidecar cannot load the server entry with plain Node.
- A Windows build given `--wsl-runtime` omits the WSL archive or SHA-256
  sidecar, or the sidecar digest does not match the emitted archive.
- The emitted WSL archive is not a Linux CLI release archive: it must unpack to
  a single `t3-<version>-linux-<arch>` directory holding `t3`, `client/`, and
  `node_modules/` with the Linux node-pty binary, and must not carry a loose
  server bundle (`bin.mjs`).
- The external Windows resource monitor is absent.
- The unpacked Windows application contains more than 80 files.

Cross-architecture Windows builds retain every structural and extracted-sidecar
check, but skip executing the target Electron binary. A same-architecture build
for each release target must exercise the primary native-load probe.

NSIS differential packaging remains enabled. A sidecar layout transition can
produce a larger one-time download; subsequent small releases retain their
blockmaps, with a 60 MB maximum for a representative sidecar-to-sidecar update.

## 1) Release validation and unsigned builds

There is no dry-run tag path. Pushing any accepted non-nightly tag, including
`v0.0.0-test.1`, classifies the run as the stable channel and creates a real GitHub Release. Do
not push a test tag to validate the workflow.

The workflow has no non-publishing `workflow_dispatch` mode. Use normal CI or local quality gates to
validate checks and builds without shipping. To exercise the complete release graph at lower stable
risk, manually dispatch `channel=nightly`; this still publishes a real GitHub
prerelease with desktop updater metadata, CLI archives, and server binaries, but it does not touch
the `latest` updater channel. Only run it when a real nightly release is acceptable.

Manual `channel=stable` with a version input is also a real stable-channel release. Omitting signing
secrets only makes platform artifacts unsigned; it does not prevent publication.

## 2) Apple signing + notarization setup (macOS)

Required secrets used by the workflow:

- `CSC_LINK`
- `CSC_KEY_PASSWORD`
- `APPLE_API_KEY`
- `APPLE_API_KEY_ID`
- `APPLE_API_ISSUER`
- `MACOS_PROVISIONING_PROFILE` (base64-encoded provisioning profile with Associated Domains)

Required repository variables:

- `APPLE_TEAM_ID`

Optional repository variables:

- `CLERK_PASSKEY_RP_DOMAINS`: comma-separated RP-domain override. By default, the build derives the
  domain from the production Clerk publishable key.

Checklist:

1. Apple Developer account access:
   - Team has rights to create Developer ID certificates.
2. Create an explicit App ID for `com.t3tools.t3code` and enable Associated Domains.
3. Create a `Developer ID Application` certificate and a compatible provisioning profile for that
   App ID with Associated Domains enabled.
4. Export the certificate + private key as `.p12` from Keychain.
5. Base64-encode the `.p12` and store as `CSC_LINK`.
6. Base64-encode the provisioning profile and store it as `MACOS_PROVISIONING_PROFILE`.
7. Store the `.p12` export password as `CSC_KEY_PASSWORD`, and set `APPLE_TEAM_ID` to the
   10-character Apple Developer Team ID.
8. In App Store Connect, create an API key (Team key).
9. Add API key values:
   - `APPLE_API_KEY`: contents of the downloaded `.p8`
   - `APPLE_API_KEY_ID`: Key ID
   - `APPLE_API_ISSUER`: Issuer ID
10. Complete the Clerk Native API and AASA setup in [T3 Connect setup](./connect-setup.md#desktop-passkeys).
11. Re-run a tag release and confirm macOS artifacts are signed/notarized and contain the expected
    `com.apple.developer.associated-domains` entitlement.

Notes:

- `APPLE_API_KEY` is stored as raw key text in secrets.
- The workflow writes it to a temporary `AuthKey_<id>.p8` file at runtime.
- The workflow decodes `MACOS_PROVISIONING_PROFILE`, validates it with `security cms`, and passes it
  to the desktop packager.

## 3) Azure Trusted Signing setup (Windows)

Required secrets used by the workflow:

- `AZURE_TENANT_ID`
- `AZURE_CLIENT_ID`
- `AZURE_CLIENT_SECRET`
- `AZURE_TRUSTED_SIGNING_ENDPOINT`
- `AZURE_TRUSTED_SIGNING_ACCOUNT_NAME`
- `AZURE_TRUSTED_SIGNING_CERTIFICATE_PROFILE_NAME`
- `AZURE_TRUSTED_SIGNING_PUBLISHER_NAME`

Checklist:

1. Create Azure Trusted Signing account and certificate profile.
2. Record ATS values:
   - Endpoint
   - Account name
   - Certificate profile name
   - Publisher name
3. Create/choose an Entra app registration (service principal).
4. Grant service principal permissions required by Trusted Signing.
5. Create a client secret for the service principal.
6. Add Azure secrets listed above in GitHub Actions secrets.
7. Re-run a tag release and confirm Windows installer is signed.

## 4) Ongoing release checklist

1. Ensure `main` is green in CI.
2. Bump app version as needed.
3. Create release tag: `vX.Y.Z`.
4. Push tag.
5. Verify workflow steps:
   - preflight passes
   - release quality checks pass
   - `build_bundle` and all platform builds pass
   - `mobile` and `server` jobs pass so the release carries mobile and server assets
   - release job uploads expected files
6. Smoke test downloaded artifacts.

## 5) Troubleshooting

- macOS build unsigned when expected signed:
  - Check all Apple secrets plus `APPLE_TEAM_ID` are populated and non-empty.
  - Confirm the provisioning profile belongs to `APPLE_TEAM_ID.club.touchtech.s5code` and includes
    Associated Domains.
- Windows build unsigned when expected signed:
  - Check all Azure ATS and auth secrets are populated and non-empty.
- Build fails with signing error:
  - Retry with secrets removed to confirm unsigned path still works.
  - Re-check certificate/profile names and tenant/client credentials.
