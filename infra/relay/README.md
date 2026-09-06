# T3 Connect Relay

> [!NOTE]
> Sign in to T3 Connect from the app under Settings > Connections.

The relay is the hosted control plane for T3 Connect. It helps clients discover and connect to
remote environments, manages the cloud-side records needed for those connections, and delivers
optional mobile notifications and Live Activities.

The relay is intentionally not in the hot path for normal T3 Code traffic. After a client connects,
regular API and WebSocket traffic goes directly between that client and the selected environment.
See the [T3 Connect architecture note](../../docs/internals/t3-connect.md) for the larger system
design.

## Responsibilities

The relay currently owns:

- Linking T3 Code environments to a cloud account.
- Provisioning and tracking managed environment endpoints.
- Issuing short-lived credentials used to connect clients to linked environments.
- Listing linked environments and registered mobile devices for an account.
- Registering mobile notification preferences and APNs tokens.
- Receiving published agent activity and delivering notifications or Live Activity updates.
- Persisting relay state and exposing relay-specific traces for diagnostics.

The environment server and relay have separate credentials and trust boundaries. Read
[Environment Authentication Profile](../../docs/internals/environment-auth.md) before changing token,
credential, or authorization behavior.

## Code Map

- [`alchemy.run.ts`](./alchemy.run.ts) defines the deployed Alchemy stack.
- [`src/worker.ts`](./src/worker.ts) wires Cloudflare bindings, runtime layers, queues, and HTTP APIs.
- [`src/http/Api.ts`](./src/http/Api.ts) contains the relay HTTP handlers and authentication
  boundaries.
- [`src/environments`](./src/environments) contains environment linking, credentials, endpoint
  provisioning, and connection flows.
- [`src/agentActivity`](./src/agentActivity) contains mobile device registration, activity state,
  APNs delivery, and queue processing.
- [`src/auth`](./src/auth) contains relay token and DPoP proof handling.
- [`src/persistence/schema.ts`](./src/persistence/schema.ts) defines persisted relay state. Keep
  schema and migration changes together.

Shared request and response schemas live in
[`packages/contracts/src/relay.ts`](../../packages/contracts/src/relay.ts). Shared client-side relay
calls live in
[`packages/client-runtime/src/relay/managedRelay.ts`](../../packages/client-runtime/src/relay/managedRelay.ts).

## Working Locally

Install dependencies from the repository root, then run relay-focused checks from this directory:

```sh
vp install
cd infra/relay
vp test run
vp run typecheck
```

To run a smaller test set while iterating:

```sh
vp test run src/environments/EnvironmentLinker.test.ts
```

Before considering a change complete, run the repository-wide checks from the root:

```sh
vp check
vp run typecheck
```

Backend changes should include tests. Prefer testing the real business logic with external
dependencies represented at their boundary rather than mocking internal behavior.

## Deployment

The relay deploys through Alchemy:

```sh
vp run --filter t3code-relay deploy
```

The stack provisions the Cloudflare Worker and queues, managed environment endpoints, a Cloudflare
Hyperdrive into the configured Postgres database, and relay tracing resources. Copy
[`infra/relay/.env.example`](./.env.example) to `infra/relay/.env` and fill in the deployment-specific
values before deploying. Alchemy loads that file from the relay directory. Runtime secrets include
Clerk and (optionally) APNs credentials. Production adopts the configured API and tunnel DNS zones
as retained Cloudflare resources. Personal stages reference the production-owned zones.

### Database (Neon, not PlanetScale)

The `prod` Alchemy stage owns the retained database and is the shared hosted relay for stable and
nightly clients. Every other stage references that same database rather than provisioning an isolated
branch — with Neon you supply a single `DATABASE_URL` in `infra/relay/.env` (a pooled or direct
standard Postgres string) and the relay points a Cloudflare Hyperdrive at its origin, so the Worker
talks to Neon through the edge instead of the public internet:

```sh
vp run --filter t3code-relay deploy -- --stage prod
vp run --filter t3code-relay deploy -- --env-file .env.local
```

**Run migrations out-of-band.** Alchemy no longer applies `./migrations/postgres` for you (that was
PlanetScale provider behavior). Apply them against the same `DATABASE_URL` once, before or right after
the first deploy, using drizzle-kit from the relay directory:

```sh
drizzle-kit migrate --dialect=postgres --url="$DATABASE_URL" --out=./migrations/postgres
```

`DATABASE_URL` belongs in the deploy environment (or `infra/relay/.env`), not in the repository.
Keep schema changes in `src/persistence/schema.ts` together with generated migrations as before.

### Optional APNs

Apple push credentials (`APNS_ENVIRONMENT`, `APNS_TEAM_ID`, `APNS_KEY_ID`, `APNS_BUNDLE_ID`,
`APNS_PRIVATE_KEY`) are optional. When any of them is absent the relay still deploys and links
then serves remote connections; only iOS mobile push notifications and Live Activities are disabled.
The worker logs `Neither APNs nor FCM configured; mobile notifications and Live Activities disabled`
when both providers are absent, and skips only the disabled provider's queue when one is configured.

### Optional FCM

Firebase Cloud Messaging credentials (`FCM_PROJECT_ID`, `FCM_CLIENT_EMAIL`, `FCM_PRIVATE_KEY`) are
optional and mirror APNs: when any of them is absent, Android mobile push notifications are disabled
while the relay still deploys normally. Get the values from the Firebase console under Project
settings -> Service accounts -> Generate new private key; use the `project_id`, `client_email`, and
`private_key` fields from the downloaded JSON.

Alchemy defaults personal deployments to the `dev_$USER` stage. Relay custom domains apply the same
DNS-safe sanitization as Alchemy physical resource names, so `prod` uses
`relay.<RELAY_API_ZONE_NAME>` and `dev_julius` uses
`relay-dev-julius.<RELAY_API_ZONE_NAME>`. Managed environment endpoints are provisioned below
`RELAY_TUNNEL_ZONE_NAME`, which may be a different Cloudflare zone. Production tunnel hostnames use
`prod-<digest>.<RELAY_TUNNEL_ZONE_NAME>`; personal stages use
`<stage>-<digest>.<RELAY_TUNNEL_ZONE_NAME>`. `RELAY_DOMAIN` remains available as an explicit API
domain override.

After a successful deploy, the wrapper updates the repository-root `.env` file with the derived relay
URL. That makes subsequent source builds point at the relay that was just deployed without copying
the URL manually.

### Deployment CI

The relay is versioned separately from client releases. `.github/workflows/deploy-relay.yml` deploys
the shared Alchemy `prod` stage on every push to `main`. Stable and nightly release builds both
resolve their static public config from the same
`production` GitHub environment. Pull requests do not deploy relay stages. Developers can
deploy personal non-production stages locally with any stage name other than `prod`.

The repository must define these Actions variables shared by relay deployments:

- `CLOUDFLARE_ACCOUNT_ID`
- `PLANETSCALE_ORGANIZATION`
- `AXIOM_ORG_ID`

The repository must define these Actions secrets shared by relay deployments:

- `CLOUDFLARE_API_TOKEN`
- `AXIOM_TOKEN`

The relay `DATABASE_URL` is supplied per-deploy (from `infra/relay/.env` or the deploy environment); it
is a Postgres connection string and should not be committed. `APNS_*` and `FCM_*` are optional (see above).

The `production` GitHub environment must define these Actions variables:

- `RELAY_API_ZONE_NAME`
- `RELAY_TUNNEL_ZONE_NAME`
- `RELAY_DOMAIN` if overriding the derived production relay domain
- `CLERK_PUBLISHABLE_KEY`
- `CLERK_JWT_AUDIENCE`
- `CLERK_JWT_TEMPLATE`
- `APNS_ENVIRONMENT`
- `APNS_TEAM_ID`
- `APNS_KEY_ID`
- `APNS_BUNDLE_ID`
- `FCM_PROJECT_ID`
- `FCM_CLIENT_EMAIL`

The `production` GitHub environment must define these Actions secrets:

- `CLERK_SECRET_KEY`
- `APNS_PRIVATE_KEY`
- `FCM_PRIVATE_KEY`

The account-scoped repository credentials are consumed by Alchemy while provisioning relay stages; they
are not bound into the relay Worker. The production deployment uses an Axiom personal access token,
so `AXIOM_ORG_ID` must accompany `AXIOM_TOKEN`. The release workflow reads the production relay's
derived public URL and Clerk publishable key from the same environment for downstream desktop, CLI,
and hosted web builds.

See:

- [T3 Connect setup](../../docs/operations/connect-setup.md) for Clerk keys, JWT templates, and sign-up restrictions.
- [Relay Observability](../../docs/operations/relay-observability.md) for deployment tracing and diagnostics.
- [T3 Connect architecture](../../docs/internals/t3-connect.md) for environment linking and trust boundaries.
