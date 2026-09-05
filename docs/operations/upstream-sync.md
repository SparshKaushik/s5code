# Syncing the fork with upstream

This is the runbook for pulling `pingdotgg/t3code` (the `og` remote) into the
S5 Code fork. It captures the decisions made during the 2026-08 sync and the
rules that keep future syncs cheap and safe.

## The one rule that matters

**Prioritize fork-specific features, then layer upstream features and fixes on
top.** The fork is a rebrand plus a set of product features (compaction,
usage tags, self-hosted relay, binary self-update). A sync must
never silently drop any of them, and must not re-introduce upstream work that
the fork deliberately removed.

## Remotes and branches

- `origin` — `github.com/SparshKaushik/s5code` (the fork).
- `og` — `github.com/pingdotgg/t3code` (upstream).
- `main` — the fork's working branch.

```bash
git fetch og --prune
```

## The merge strategy

Use a **merge commit**, not a rebase. A rebase rewrites the fork's history and
forces a push; a merge keeps the fork's history intact, is reversible, and
produces a single auditable commit.

```bash
git branch backup/pre-upstream-merge-$(date +%Y%m%d)   # safety snapshot
git merge --no-commit --no-ff og/main
# resolve conflicts, then:
git commit -m "merge: sync with upstream pingdotgg/t3code, keep fork features"
```

### Resolving conflicts

Conflicts fall into three buckets:

1. **modify/delete** (`DU` in `git status --porcelain`) — the fork deleted a
   file upstream modified. These are almost always upstream **pull-request
   feature** files the fork dropped. Resolve by taking upstream's version
   (`git checkout og/main -- <file>`) when re-adopting PR, or keep the deletion
   when the file is a fork-intentional removal (see below).

2. **content** (`UU`) — both sides edited the same file. Reconcile by hand,
   keeping the fork's rebrand/feature content and upstream's fixes.

3. **add/add** (`AA`) — both sides added a file. Merge the content.

## Migrations: preserve ordering, `+1` the incoming

The server's SQLite migrations live in
`apps/server/src/persistence/Migrations/` and are keyed by a numeric ID in
`Migrations.ts`. Migrations 001–040 are in sync with upstream `t3code`
(the previous experimental `038_RewindEntries` migration was removed, and
automatic recovery realigns existing user databases).

**Rule:** when upstream ships new migrations, adopt their numeric IDs when
cleanly tracking upstream. Any new fork-specific migrations should use `+1`
past the highest ID.

The relay migrations in `infra/relay/migrations/postgres/` are timestamped and
do not collide; leave them alone.

## Fork-specific features to preserve

These are the things a sync must never regress. Grep for them after a merge:

- **Rebrand** — `s5code://` scheme, `~/.s5code` home dir, `app.s5code.touchtech.club`,
  self-hosted Clerk/relay URLs, "S5 Code" copy. Files: `apps/desktop`,
  `apps/mobile/app.config.ts`, `apps/web/index.html`, `packages/shared/src/devHome.ts`.
- **Compaction** — `/compact` command (pi harness).
- **Usage tags** — `UsageModelAlias`, `UsageCatalogModelId`, `userTagged` cost
  source, `pi`/`cursor` provider kinds, `UsagePricer`. Files:
  `packages/contracts/src/usage.ts`, `apps/server/src/usage/usagePricing.ts`,
  `apps/web/src/components/usage/`.
- **Binary self-update** — `"binary"` in `ServerSelfUpdateMethod`/`Capability`,
  `apps/server/src/cloud/binaryUpdate.ts`.
- **Self-hosted relay** — `infra/relay/`, `.github/workflows/deploy-relay.yml`.

## Fork-intentional deletions (do NOT restore)

These upstream files were removed on purpose. A sync that re-adds them is a bug:

- `.github/workflows/mobile-fingerprint-check.yml` (fork has its own CI).
- `apps/mobile/plugins/withAndroidTabletOrientation.cjs`.
- `apps/mobile/src/components/T3Wordmark.tsx` (rebrand).
- `apps/mobile/src/features/threads/GitActionProgressOverlay.tsx`.
- `infra/relay/src/dbConfig.ts` (+ test) — fork's relay rewrite.

## The pull-request feature

Upstream's pull-request feature is a **web of interdependent files**. Re-adopting
it is not just restoring `apps/server/src/pullRequest/` and
`apps/web/src/components/pullRequest/`; it also requires the newer versions of:

- `apps/server/src/sourceControl/*` (the `request` method on `BitbucketApi`,
  `stdin`/`maxOutputBytes` on the CLIs).
- `apps/server/src/vcs/VcsProcess.ts` (`stdoutInvalidUtf8`).
- `apps/server/src/processRunner.ts` + `stream/collectUint8StreamText.ts`
  (`decodeUtf8`, `invalidUtf8`).
- `packages/contracts/src/{pullRequest,ipc,environment,environmentHttp,index}.ts`
  (`ConfirmDialogOptions`, `pullRequests` capability, the `pullRequests` HTTP group).
- `packages/client-runtime/src/{rpc/http,runtime,state/pullRequests,state/pullRequestDiffHttp}.ts`.
- `apps/web/src/{ChatView,Sidebar,RightPanelTabs,rightPanelStore,sourceControlPresentation,state/queries,...}`.

The previous sync (commit `213e572db`) dropped this whole web as "incompatible"
because piecemeal adoption broke typecheck. Re-adopting it means taking
upstream's versions of the whole web and re-applying the fork's rebrand/usage
changes on top.

### Known remaining divergences (2026-08 sync)

The web re-adoption is not fully green. These fork-vs-upstream divergences need
a human decision before they can be closed:

- **Settings routes** — the fork uses `settings/projects` +
  `settings/projects_/$projectKey`; upstream uses `projects/$projectKey`. The
  fork's routes pass `selectedProjectKey`, upstream's `ProjectSettingsPanel`
  takes `projectKey`.
- **Connection runtime** — `apps/web/src/state/pullRequests.ts` calls
  `createPullRequestEnvironmentAtoms(connectionAtomRuntime)`; the fork's
  `connection/runtime.ts` is missing a service upstream's client-runtime expects.
- **Git service** — upstream's `GitManager` adds `isOnPullRequestHead`, which
  depends on `resolveCommit`/`fetchPullRequestHeadCommit`/`refreshCheckedOutBranch`
  on the git service the fork doesn't have.

## Verification

After a merge, run targeted typechecks (never the repo-wide `vp check`):

```bash
vp run --filter t3 typecheck
vp run --filter @t3tools/contracts typecheck
vp run --filter @t3tools/shared typecheck
vp run --filter @t3tools/client-runtime typecheck
vp run --filter @t3tools/web typecheck
```

Regenerate the TanStack route tree when routes change (it happens as a side
effect of `vp run --filter @t3tools/web build`; the build also typechecks).

Backend behavior changes ship with focused tests (`vp test run <files>`).
