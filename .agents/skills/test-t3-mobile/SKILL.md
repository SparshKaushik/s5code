---
name: test-t3-mobile
description: Launch and test S5 Code Mobile on an iOS Simulator or Android Emulator against disposable local T3 environments, including Metro and dev-client reuse, native rebuild decisions, per-client pairing, seeded projects, semantic UI control, screenshots, and iOS serve-sim streaming. Use after mobile UI or native changes, when reproducing phone or tablet behavior, pairing an emulator to isolated state, or verifying mobile behavior on macOS, Linux, or Windows.
---

# Test T3 Mobile

## Open the device

Call `device_list`, then `device_open` with the selected host and device IDs.
T3 boots the device and shows its live stream in the Device panel. Follow its
returned `quickStart`, using the exact `agentDevice.command` and all `targetArgs`
on every operation. Use `device_screenshot` to inspect the screen.

If T3 device tools or the selected device are unavailable, report the blocker
and stop verification. Do not install or switch to another automation system.

## Use an isolated backend

Reuse this task's healthy backend. Otherwise run `vp run dev` from the
repository root, retain its terminal session, and read the actual backend port
from the dev-runner output. Use the worktree's ignored `.t3` state. Never run
against `~/.t3/userdata`. The Browser panel is not required for this workflow.

Test with meaningful project and thread data. Read the shared
[SQLite fixture reference](../test-t3-app/references/sqlite-fixtures.md) only
when inspecting or seeding SQLite. Stop the test server before fixture writes.

## Launch T3 Code Dev

From the checkout being tested on the selected device host, run:

```bash
node scripts/mobile-native-client.ts ensure <ios|android> <device-id>
```

This reuses a matching native client or builds and installs one. Authorized
mobile verification includes that build step unless the user prohibits it.

The development identity on both platforms is:

- App: `S5 Code Dev`
- Bundle/package identifier: `club.touchtech.s5code.dev`
- URL scheme: `s5code-dev`

Bundle or package presence proves the correct variant, not native compatibility. Reuse it only when the current changes did not alter its Expo SDK, native dependencies, config plugins, entitlements, generated project, or native source.

## Start one disposable T3 environment

Run backend commands from the repository root. Use the ignored, worktree-local `.t3` directory or create a fresh directory with the host OS's temporary-directory mechanism. An explicit base directory stores state in `<base-dir>/userdata`; never point testing at shared `~/.t3` state.

Seed a small number of meaningful Git projects before starting the backend:

```bash
node apps/server/src/bin.ts serve \
  --host 127.0.0.1 \
  --port <server-port> \
  --base-dir <base-dir> \
  --no-browser
```

Use these client origins:

- iOS Simulator: `http://127.0.0.1:<server-port>`
- Android Emulator: `http://10.0.2.2:<server-port>`
- Physical device: bind the backend to `0.0.0.0` and use the host's reachable LAN origin

Enter the complete `http://` origin to make the test transport explicit. Bare IP addresses default to HTTP, while bare hostnames default to HTTPS. When testing web and mobile together, run `vp run dev --home-dir <base-dir> --host 127.0.0.1` instead and do not launch a second backend over the same base directory.

## Start or reuse Metro safely

Run Metro from `apps/mobile`.

1. Inspect any process on the intended Metro port and its `/status` response. Reuse it only when it is healthy, belongs to this worktree, and matches `APP_VARIANT=development`, `--dev-client`, and scheme `s5code-dev`.
2. Never kill another worktree's Metro. Use a free explicit port when necessary.
3. Run `vp run dev:client` on the standard port. For another port, retain the complete development identity:

   ```bash
   APP_VARIANT=development vp exec expo start \
     --dev-client \
     --scheme s5code-dev \
     --lan \
     --port <metro-port>
   ```

   In PowerShell, set `$env:APP_VARIANT = "development"` first and then run the `vp exec expo start ...` command without the leading assignment.

4. Open the exact development-client URL for the selected device and confirm the loaded bundle belongs to this worktree and Metro port.

### iOS launch

Use `ios-debugger-agent` to select one UDID and set these XcodeBuildMCP session defaults:

- Workspace: `<repo>/apps/mobile/ios/T3CodeDev.xcworkspace`
- Scheme: `T3CodeDev`
- Configuration: `Debug`
- Simulator ID: the selected UDID
- Bundle ID: `club.touchtech.s5code.dev`

Check the installed client with:

```bash
xcrun simctl get_app_container <simulator-udid> club.touchtech.s5code.dev app
xcrun simctl openurl <simulator-udid> <printed-dev-client-url>
```

Accept the iOS confirmation prompt and dismiss the developer menu when it obscures the app.

### Android launch

Select one running emulator serial from `adb devices` and check the installed client:

```bash
adb -s <emulator-serial> shell pm path club.touchtech.s5code.dev
adb -s <emulator-serial> reverse tcp:<metro-port> tcp:<metro-port>
adb -s <emulator-serial> shell am start -W \
  -a android.intent.action.VIEW \
  -d '<printed-dev-client-url>' \
  club.touchtech.s5code.dev
```

Do not start, stop, erase, or reconfigure an emulator owned by another task. Track and later stop only processes owned by this test.

## Pair each client once

Use the bundled helper from the repository root. It issues a fresh credential against the running backend's exact base directory, opens the existing Add Environment route with the credential in an encoded query parameter, and asks that route to connect once:

```bash
.agents/skills/test-t3-mobile/scripts/pair-client.sh \
  <ios|android> <device-id> <server-port> <base-dir> [url-scheme]
```

The helper opens this registered route:

```bash
xcrun simctl openurl <simulator-udid> 's5code-dev://connections/new'
adb -s <emulator-serial> shell am start -W \
  -a android.intent.action.VIEW \
  -d 's5code-dev://connections/new' \
  club.touchtech.s5code.dev
```

Alternatively, use the deterministic one-shot pairing route, which carries the pairing URL and skips the visible form:

```text
s5code-dev://connections/new?pairingUrl=<encoded-pairing-url>&autoConnect=1
```

The Add Environment route owns the behavior: `pairingUrl` prefills its normal host and token inputs, while `autoConnect=1` submits once in development builds and returns to Home after success. Without `autoConnect`, the same route only prefills the form for manual inspection.

In S5 Code Dev, open Add Environment and enter the complete `<mobile-origin>` and newly printed `Token`. Verify the expected seeded projects appear before exercising the affected flow.

Do not enter pairing hosts or tokens through simulator keyboard automation. Xcode's semantic typer sends HID-style key events through the simulator's active keyboard state, which can corrupt uppercase tokens and punctuation even when the host Mac uses a U.S. input source. The one-shot route is the deterministic pairing path. Use the visible form only as a fallback, and paste credentials rather than typing them character by character.

Pairing credentials are secret, short-lived, and single-use. Create a different credential for every simulator, emulator, physical device, or browser. If an attempt fails, issue a new credential rather than retrying the old one. Do not expose tokens in screenshots, commits, or final responses.

## Drive and observe the affected flow

### iOS

Use `snapshot_ui` and current element references from XcodeBuildMCP for taps and typing. Stream the same UDID through `ios-simulator-browser` so the user can watch in S5 Code when the host supports it. Use the stream as a visual feed rather than a reason to switch to fragile browser coordinates.

### Android

Prefer semantic Android automation exposed by the current agent host. Otherwise inspect the current hierarchy with `adb shell uiautomator dump`, target stable resource IDs, content descriptions, text, or bounds, and use scoped `adb shell input` actions. Refresh the hierarchy after navigation. Capture the final state with `adb exec-out screencap -p`.

Android does not use serve-sim. Use a browser-compatible Android mirror when the host already provides one; otherwise return focused emulator screenshots as evidence rather than installing unrelated streaming infrastructure during verification.

## Verify and clean up

Exercise only the affected flow on one representative device unless the change specifically concerns platform, OS version, or screen size. Before finishing:

1. Confirm the app connected to the intended disposable environment instead of merely rendering an empty disconnected state.
2. Capture the relevant final state.
3. Remove the disposable environment from S5 Code Dev.
4. Remove any `adb reverse` rule created for this test with `adb -s <emulator-serial> reverse --remove tcp:<metro-port>`.
5. Stop only the serve-sim, Metro, backend, emulator, and log processes started by this test.
6. Remove only base directories and temporary Git repositories deliberately created for this test. Preserve them when they contain useful reproduction evidence.

Keep local verification focused. Do not turn this workflow into a full repository test run.

## Troubleshoot predictable failures

- **Old UI or an old error appears:** verify Metro's worktree, variant, URL, and port before diagnosing the app.
- **Metro serves stale or invalid transforms after those checks:** stop the owned Metro process and run `vp run dev:client:reset` once on the standard port. For a custom port, add `--clear` to the complete explicit `expo start` command above.
- **The environment remains empty:** verify the platform-specific HTTP origin, use a fresh token, and confirm project seeding used the identical base directory.
- **A second client cannot pair:** pairing tokens are single-use; issue another token.
- **The pairing form opens but does not connect:** confirm the deep link uses the existing `connections/new` route, includes `autoConnect=1`, and carries a freshly minted encoded `pairingUrl`.
- **Pairing text changes case or punctuation:** do not retry semantic typing. Use `scripts/pair-client.sh`; the simulator keyboard layout and HID input path are not reliable for credentials.
- **iOS semantic actions fail:** set explicit XcodeBuildMCP defaults and refresh with `snapshot_ui`.
- **Android cannot reach Metro:** verify `adb reverse` for the exact Metro port and relaunch the development-client URL.
- **Android cannot reach the backend:** use `10.0.2.2`, not `127.0.0.1`, for the Android Emulator.
