# Keeping S5 Code in Sync

The S5 Code web or desktop app and the server it connects to work best when they use the same
version. If they do not match, S5 Code shows a warning with the right update option for that server.

## Where to Find the Update

You may see the warning in either of these places:

- above the message box in the current conversation
- **Settings** → **Connections**, beside the affected connection

Dismissing the conversation warning only hides that reminder for those two versions. It does not
update the server, and the version difference remains visible in Connections.

## Desktop App Updates

The desktop app downloads the release package for its current channel and asks you to install it.
Installing an update stops local backends and briefly closes the app before reopening the new
version.

On macOS, Developer ID-signed builds use the native updater. Ad-hoc signed development and alpha
builds use the same downloaded ZIP but replace the app bundle directly, so they can update without
an Apple Developer certificate. Keep the app in a user-writable location such as `/Applications`.
If replacement cannot start, S5 Code restores the current app, restarts its backends, and leaves the
update available to retry.

## Before You Update

Let active agent work and terminal commands finish first. Updating restarts the server, so the
connection will disappear briefly and work that is still running may be interrupted.

The update does not remove saved threads, settings, or project files.

## Choose the Action You See

| Action                     | What to do                                                                                                                                                                                 |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Update server**          | Available for the S5 Code Linux background service and for precompiled Linux server binaries. Select the button and leave S5 Code open while it prepares, tests, restarts, and reconnects. |
| **Update the desktop app** | Open the S5 Code desktop app on the machine that runs the server and install the app update there. Reopen it if needed.                                                                    |
| **Copy update command**    | Copy the command, open a terminal on the server machine, stop the current S5 Code server, and relaunch it with the copied command and any startup options you normally use.                |

The available action depends on how that server was started. S5 Code does not update connected
servers silently in the background.

An older background-service launcher may ask you to run the exact
`npx t3@<version> service update` command on the server machine. That one local update installs the
rollback support needed for later remote updates, including versions that change the database.

A **precompiled Linux server binary** updates the same way through that button: it downloads the
release build for its own version and processor, checks that the download runs and reports the
version you asked for, then replaces itself and restarts. If the download is missing, incomplete,
or built for a different processor, the update stops and the server keeps running the version it
already had.

After selecting **Update**, the notice becomes a live status line: **Downloading…** while the new
version is fetched and verified, then **Restarting…** while the server restarts into it. The same
status appears in the conversation and in Connections, so navigating between them does not lose the
update. A failure remains visible with its error and an option to retry.

**Copy update command** gives you `npx t3@<client-version>`, which relaunches the server directly
at the matching version. Add whatever startup options you normally use.

If the server instead runs as the S5 Code background service, update the service on the host and
pin the same version:

```sh
npx t3@<client-version> service update
```

`service update` installs the version of the CLI that invoked it, so `npx t3@latest service update`
only resolves the skew when your client happens to be on the latest release. The exact version from
the warning always works.

See [Running S5 Code in the Background](./background-service.md) for install, status, and removal
commands.

## After the Update

Keep the web or desktop app open while the server restarts. The update completes only after the
service launcher reports that exact update committed and the replacement server is ready to accept
commands. A rollback is reported immediately instead of waiting for a generic reconnect timeout.

If a step fails:

1. Retry the offered action once.
2. Make sure you updated the machine named in the warning, not only the device you are using.
3. For a command-line server, relaunch it with `npx t3@<client-version>`, replacing
   `<client-version>` with the client version shown in the warning.

## The Mobile App

The mobile app keeps itself current on its own. When it finds a new version, it downloads it in the
background and installs it automatically the next time you leave the app. Unsent drafts and queued
messages are saved before the restart. Only if the app stays open long enough that the update never
gets that chance does it ask whether to install right away; choosing **Later** is safe and keeps the
automatic install armed.

For remote connection setup and access troubleshooting, see [Remote Access](./remote-access.md).
