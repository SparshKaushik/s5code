# Server updates

A [stable launcher](../../apps/server/src/serviceLauncher.ts) owns the runtime
selected by systemd or launchd. It is the only runtime writer of durable service
state. Server children request updates over inherited IPC; they never rewrite
their service definition or select their own replacement. Local service commands
may replace the launcher and state while the service is stopped. Foreground CLI
processes do not self-update.

Exact-version installs keep restarts independent of npm cache eviction or a moving
release tag. Installation and preflight happen in staging before publishing an
immutable runtime. Preflight checks the launcher protocol because a target that
needs new rollback guarantees cannot safely run under an older launcher. Upgrading
that launcher requires a local service update.

## Commit boundary

The launcher durably records the pending update before acknowledging it, then
stops the old child and starts the target as a trial. Service-state writes use
same-directory replacement with file and directory fsync. Invalid state stops
startup rather than guessing which runtime to boot.

The trial must finish migrations, acquire dependencies, bind HTTP, and park every
long-running root at the activation gate before reporting `prepared`. The launcher
then commits the target version durably and replies `committed`. Only then may the
child release its gates, accept commands, and publish ready. Keep fallible startup
acquisitions before this boundary. A listener alone does not prove the runtime is
ready to commit.

A failed or timed-out trial returns to the old version. After commit, the target
is authoritative and the service manager's ordinary restart policy applies.

## Database rollback

After the old child exits, the launcher snapshots SQLite's main file, WAL, and
shared-memory file. This makes trial migrations reversible without down
migrations. The snapshot is made once per update and survives launcher restarts;
replacing it during a retry could capture changes from the failed trial.

Rollback stops the trial before restoring. A durable restore marker makes an
interrupted restore finish before either version boots. Keep the snapshot until
commit, or until both restoration and the terminal rollback state are durable.
Attachments and other files outside SQLite are outside this rollback boundary.

## Client acknowledgement

An accepted update is still pending. Clients correlate the launcher's update ID
with the ready event after reconnecting, then check the outcome and target version.
A reconnect alone cannot distinguish successful replacement from rollback. Older
servers without an update ID retain version-only correlation.

## Release Binary Updates

A release binary (`scripts/build-server-binary.ts`, `bun build --compile`) has no npm tree to stage
into and no launcher, so it takes a separate path. Internally it selects the `binary` update method
from runtime identity; the public environment descriptor still advertises `boot-service` until
production `relay.t3.codes` (and app clients) decode the `binary` capability — otherwise link-proof
JWTs fail schema validation as `environment_link_proof_invalid`.

It recognizes itself from two facts, both required: `process.argv[1]` starts with `/$bunfs/` (Bun's
virtual entry point, the only reliable standalone signal on the pinned Bun), and the build inlined
`T3CODE_SERVER_BINARY_TARGET` plus `T3CODE_SERVER_BINARY_REPO` via `--define`. A half-configured
build resolves to no identity, so it gets no update path rather than a broken one.

Those two reads must stay written as literal `process.env.<NAME>` member expressions at module
scope, because that is the only form bun's `--define` rewrites. Reading them off a passed-around
`process.env` leaves the lookup dynamic, the values never apply, and every binary silently resolves
to no identity. The build script asserts the inline landed by requiring both variable _names_ to be
absent from the compiled output, and fails the build otherwise.

Release CI builds each Linux arch on a matching runner (`ubuntu-24.04` / `ubuntu-24.04-arm`).
Cross-compiling `bun-linux-arm64` on x64 can produce a binary that omits
`@yuuang/ffi-rs-linux-arm64-gnu` (required by `@ff-labs/fff-node`) and still exits the compile
step cleanly; the host-arch `--version` smoke test in `scripts/build-server-binary.ts` is what
catches that before publish.

The update is:

1. Download `https://github.com/<repo>/releases/download/v<target>/s5code-server-<target>-<arch>`.
   The plain download route needs no API call or token, so a host with only HTTPS egress can update.
2. Write it to a staged path in the executable's own directory and `chmod 0755`.
3. Run the staged file with `--version` and require exit 0 and the requested version in stdout. A
   truncated download, wrong architecture, or glibc mismatch fails here rather than at next boot.
4. `rename` the staged file over the running executable. This is atomic on POSIX and legal while the
   old image is executing: the running process keeps its open inode, only the directory entry moves.
5. Acknowledge the RPC, then restart. Under systemd (the launcher-installed unit sets
   `T3_BOOT_SERVICE_UNIT`, and systemd itself sets `INVOCATION_ID` for every unit invocation, which
   covers hand-written units) the process exits non-zero and `Restart=always` starts the
   replacement, keeping systemd the sole owner. Unsupervised, it spawns the replacement detached
   and exits cleanly. The self-exec path is never chosen under systemd: the default
   `KillMode=control-group` would kill the detached replacement along with the exiting unit.

Any failure after the write removes the staged artifact and leaves the running binary in place.
There is no launcher and therefore no database snapshot: the replacement runs its own normal
startup, including migrations, and a version that migrates the database cannot be rolled back
automatically.

## Desktop updates

Desktop updates have a separate two-phase handoff because installing the app stops
its bundled backend. Preparation returns a token while the connection is alive;
the client commits that token only after receiving it. Otherwise backend shutdown
could lose the only successful RPC result. The client must then observe the
prepared version after reconnecting. If installation fails, desktop restarts the
stopped backends and replays the failure for the same token.
