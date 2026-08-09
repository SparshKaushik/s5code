# Desktop Update Install Recovery Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Failed desktop update installs restart backends and restore the main window instead of leaving a dock-only process.

**Architecture:** Change `installDownloadedUpdate` teardown and shared install-failure recovery in `DesktopUpdates.ts`. Keep backend stop; drop window destroy; recover via pool `start` + `DesktopWindow.revealOrCreateMain`.

**Tech Stack:** Effect, Vitest (`@effect/vitest`), existing desktop updater harness.

---

### Task 1: Failing tests for install recovery

**Files:**

- Modify: `apps/desktop/src/updates/DesktopUpdates.test.ts`

Steps:

1. Extend harness to track `start` / `destroyAll` / `revealOrCreateMain` and stub `DesktopWindow`.
2. Add test: `quitAndInstall` fails → backends restarted, destroyAll not called, quitting cleared.
3. Add test: updater `error` while install in flight → same recovery.
4. Run `vp test run apps/desktop/src/updates/DesktopUpdates.test.ts` and confirm new tests fail.

### Task 2: Implement recovery + quarantine clear + cause logging

**Files:**

- Modify: `apps/desktop/src/updates/DesktopUpdates.ts`
- Modify: `apps/desktop/src/main.ts` if layer provide order needs `DesktopWindow` for updates

Steps:

1. Remove `destroyAll` from install path.
2. Add `recoverAfterInstallFailure` used by sync catches and `handleUpdaterError` install-in-flight path.
3. Log cause strings with updater errors.
4. Clear quarantine on darwin packaged apps before `quitAndInstall`.
5. Re-run tests until green.

### Task 3: Verify

Run: `vp test run apps/desktop/src/updates/DesktopUpdates.test.ts`
