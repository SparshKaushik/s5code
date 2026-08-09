# Desktop update install recovery

## Problem

In-app `quitAndInstall` can fail after backends are stopped (and previously after all windows were destroyed). Failure handling only cleared flags, leaving a dock-only process with no GUI.

## Design (approach B)

1. Stop backends before install (keep — needed for clean child shutdown).
2. Do not call `destroyAll` before `quitAndInstall`; Electron quit handles windows on success.
3. On any install failure (sync catch or async updater `error` while install in flight):
   - clear install/`quitting` flags
   - restart all pool backends
   - reveal or recreate the main window
4. Log nested updater error causes.
5. On darwin packaged builds, clear `com.apple.quarantine` on the app bundle before `quitAndInstall`.

## Out of scope

Release signing / notarization pipeline changes.
