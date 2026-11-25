# Telemetry

S5 Code collects and exports no telemetry.

- Product usage analytics (`ingest.t3.codes` / PostHog) are removed.
- Agent feedback reporting is removed.
- Relay connection tracing (Axiom OTLP) is removed across the server, CLI, desktop, web, mobile, and relay worker.
- Mobile crash and startup reporting (`expo-observe`) is removed.
- Remote OTLP trace, metric, and log export is removed. Diagnostics stay strictly local on disk (`server.trace.ndjson` and `desktop.trace.ndjson`).

## Agent activity for mobile notifications

When **Publish agent activity** is enabled on a host, it sends project and thread titles, model
names, activity status, and environment and thread identifiers to the configured relay for
[mobile notifications and live activities](./mobile-notifications.md). To stop publishing, turn off
**Publish agent activity** under **Settings > Sharing & access** on the host.
