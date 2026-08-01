# Provider architecture

The web app communicates with the server via WebSocket using a simple JSON-RPC-style protocol:

- **Request/Response**: `{ id, method, params }` → `{ id, result }` or `{ id, error }`
- **Push events**: typed envelopes with `channel`, `sequence` (monotonic per connection), and channel-specific `data`

Push channels: `server.welcome`, `server.configUpdated`, `terminal.event`, `orchestration.domainEvent`. Payloads are schema-validated at the transport boundary (`wsTransport.ts`). Decode failures produce structured `WsDecodeDiagnostic` with `code`, `reason`, and path info.

Methods mirror the `NativeApi` interface defined in `@t3tools/contracts`:

- `providers.startSession`, `providers.sendTurn`, `providers.interruptTurn`
- `providers.respondToRequest`, `providers.stopSession`
- `shell.openInEditor`, `server.getConfig`

Each provider ships as a driver under `apps/server/src/provider/Drivers/`: Codex, Claude, Cursor, Grok, OpenCode, and pi. A driver owns config decoding, the availability snapshot, one adapter per instance, and text generation.

## The pi driver

pi (`@earendil-works/pi-coding-agent`) is driven over `pi --mode rpc`, a JSONL protocol on stdin/stdout. Three things about it differ from the other CLI-backed providers and shape the code:

- **No fixed model catalog.** What a user can run depends on their own pi configuration, so the provider snapshot spawns a short-lived `pi --mode rpc --no-session` and reads `get_available_models`. An empty catalog is the only signal pi gives that nothing is authenticated, and is reported as such. Slugs are `<pi provider>/<model id>`, split on the first separator because some pi model ids contain slashes.
- **No permission protocol.** pi tools just run. The only interactive channel a pi process has is `ctx.ui.*` from an extension, which in RPC mode becomes a blocking `extension_ui_request`. Runtime modes below `full-access` are therefore enforced from inside pi by the bundled `apps/server/pi-extension/t3-runtime-mode.ts`, loaded with `--extension` and configured through `T3CODE_PI_RUNTIME_MODE`. `confirm` requests map onto T3's approval surface; `select` and `input` map onto structured user input.
- **No plan channel.** Task lists come from todo tool _results_ (`todowrite` / `patchtodo`), which carry the whole reconciled list even when the call patched one field.

Instance isolation uses `PI_CODING_AGENT_DIR`, never `HOME`: overriding `HOME` also relocates the macOS keychain lookup and breaks pi's stored credentials. Threads resume by pi session file path, recorded in the durable resume cursor; a rollback uses pi's `fork`, which re-roots the session before a chosen user entry.

## Client transport

`wsTransport.ts` manages connection state: `connecting` → `open` → `reconnecting` → `closed` → `disposed`. Outbound requests are queued while disconnected and flushed on reconnect. Inbound pushes are decoded and validated at the boundary, then cached per channel. Subscribers can opt into `replayLatest` to receive the last push on subscribe.

## Server-side orchestration layers

Provider runtime events flow through queue-based workers:

1. **ProviderRuntimeIngestion** — consumes provider runtime streams, emits orchestration commands
2. **ProviderCommandReactor** — reacts to orchestration intent events, dispatches provider calls
3. **CheckpointReactor** — captures git checkpoints on turn start/complete, publishes runtime receipts

All three use `DrainableWorker` internally and expose `drain()` for deterministic test synchronization.
