# SQLite schema recovery and legacy home adoption

## Problem

Effect’s migrator records migrations by numeric ID. Forks and older builds reused IDs for different names (`35 = RewindEntries` vs `35 = ProjectionThreadTitleRegeneration`). On upgrade the migrator skips anything at or below the latest ID, so required columns never appear. The backend crash-loops (`no such column: …`) and the desktop shell stays in the Dock with no window.

0.2.0 also moved the default home from `~/.t3` to `~/.s5code`. A packaged app with an empty/unusable `~/.s5code/userdata` starts from a sparse DB even when the user’s real data is still in `~/.t3`.

## Design

Two independent recoveries:

1. **Desktop home adoption** (before backend spawn)
   - Only when `T3CODE_HOME` is unset, the app is packaged production (not `~/.s5code/dev`), `~/.s5code/userdata` has no `state.sqlite`, `~/.t3/userdata/state.sqlite` is a readable SQLite file, and `~/.s5code/.adopted-from-t3` is absent.
   - Copy missing files from `.t3/userdata` into `.s5code/userdata` without overwriting existing dest files. Write the marker. Do not delete `~/.t3`.
   - If `.s5code/userdata/state.sqlite` already exists, leave it (schema ensure handles collisions).

2. **Server schema ensure** (after a full `runMigrations()`, before serving)
   - Declarative checklist of additive objects from migrations 35–41 (columns, tables, indexes).
   - Create anything missing (`ALTER TABLE … ADD COLUMN`, `CREATE TABLE/INDEX IF NOT EXISTS`).
   - Never drop, rename, change types, or backfill from events.
   - Skip the ensure pass when `toMigrationInclusive` is set so incremental migration tests stay valid.
   - Log healed object names. Fail startup only if heal SQL fails.

## Out of scope

Choosing between two populated homes. Rewriting existing columns. Migrating Electron `Library/Application Support/t3code`.
