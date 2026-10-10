# Syncing the fork with upstream

This is the runbook for pulling `pingdotgg/t3code` into the S5 Code fork.

## The one rule that matters

**Follow the agreed feature scope.** Providers, chat, mobile behavior, and
checkpointing now track upstream. Retain S5 usage reporting, distribution,
infrastructure, and desktop customizations. S5 identity and deployment
configuration apply to every client. Fork checkpoint retention and storage
measurement/manual cleanup have been retired.

## Remotes and branches

- `origin` — `github.com/SparshKaushik/s5code` (the fork).
- `upstream` — `github.com/pingdotgg/t3code` (upstream).
- `main` — the fork's working branch.

```bash
git fetch upstream --prune
```

## The merge strategy

Use a **merge commit**, not a rebase. A rebase rewrites the fork's history and
forces a push; a merge keeps the fork's history intact, is reversible, and
produces a single auditable commit.

```bash
git branch backup/pre-upstream-merge-$(date +%Y%m%d)   # safety snapshot
git merge --no-commit --no-ff upstream/main
# resolve conflicts, then:
git commit -m "merge: sync with upstream pingdotgg/t3code, keep fork features"
```

### Resolving conflicts

Conflicts fall into three buckets:

1. **modify/delete** (`DU` in `git status --porcelain`) — the fork deleted a
   file upstream modified. Take upstream's version when it belongs to the
   agreed adoption scope, or keep the deletion when it is an intentional
   distribution or infrastructure choice (see below).

2. **content** (`UU`) — both sides edited the same file. Reconcile by hand,
   retaining the fork customizations in scope and upstream behavior elsewhere.

3. **add/add** (`AA`) — both sides added a file. Merge the content.

## Migrations: preserve upstream numbering

The server's SQLite migrations live in
`apps/server/src/persistence/Migrations/` and are keyed by a numeric ID in
`Migrations.ts`. Released S5 databases have migration-ID collisions with
upstream; the upgrade reconciles their ledger and heals the legacy schema
before importing into V2. Do not shift upstream migration IDs to preserve an
old fork ID: the migrator skips IDs already recorded, which can silently
skip the V2 schema itself.

The V2 upgrade snapshots `state.sqlite` into `statev2.sqlite` without
migrating the original. Existing V2 state wins on subsequent launches.
Keep this one-way copy intact so switching versions preserves both histories.

The relay migrations in `infra/relay/migrations/postgres/` are timestamped and
do not collide; leave them alone.

## Fork-specific features to preserve

These are the things a sync must never regress. Grep for them after a merge:

- **Rebrand** — `s5code://` scheme, `~/.s5code` home dir, `app.s5code.touchtech.club`,
  self-hosted Clerk/relay URLs, "S5 Code" copy. Files: `apps/desktop`,
  `apps/mobile/app.config.ts`, `apps/web/index.html`, `packages/shared/src/devHome.ts`.
- **Usage tags** — `UsageModelAlias`, `UsageCatalogModelId`, `userTagged` cost
  source, `pi`/`opencode` provider kinds, `UsagePricer`. Usage readers now live with their
  drivers in `packages/provider-pi` and `packages/provider-opencode`; preserve
  their per-instance roots and gateway metadata when moving them. Files:
  `packages/contracts/src/usage.ts`, `apps/server/src/usage/usagePricing.ts`,
  `apps/web/src/components/usage/`. Preserve gateway-specific models.dev prices,
  aliases, historical Pi usage, and estimated input/cache tokens alongside
  upstream's speed tiers and category-cost breakdowns.
- **Binary self-update** — `"binary"` in `ServerSelfUpdateMethod`/`Capability`,
  `apps/server/src/cloud/binaryUpdate.ts`.
- **Self-hosted relay** — `infra/relay/`, `.github/workflows/deploy-relay.yml`.

## Fork-intentional deletions (do NOT restore)

These upstream files were removed on purpose. A sync that re-adds them is a bug:

- `.github/workflows/mobile-fingerprint-check.yml` (fork has its own CI).
- `apps/mobile/plugins/withAndroidTabletOrientation.cjs`.
- `apps/mobile/src/components/T3Wordmark.tsx` (rebrand).
- `infra/relay/src/dbConfig.ts` (+ test) — fork's relay rewrite.

## Verification

Push the integration branch under `upstream-sync/**` to run GitHub CI. It
verifies typechecking, tests, lint, builds, and standalone Bun compatibility
without loading the contributor's machine with a repository-wide typecheck.

```bash
git push origin HEAD:upstream-sync/<sync-name>
gh run list --branch upstream-sync/<sync-name>
gh run watch <run-id> --exit-status
```

Regenerate the TanStack route tree when routes change (it happens as a side
effect of `vp run --filter @t3tools/web build`; the build also typechecks).

Backend behavior changes ship with focused tests (`vp test run <files>`).
