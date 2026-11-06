# Porting the desktop client to Rune

This crate is the S5 Code desktop client rebuilt natively on [Rune](https://github.com/anomalyco/rune).
The goal is a 1:1 copy: a user should not be able to tell it from the Electron app by looking.
The web client (`apps/web`) is the spec. Port what it renders, not what you think it should render.

## Layout

| Path | What |
| --- | --- |
| `net/` (`s5-net`) | Server protocol: Effect-RPC over WebSocket, the shell/config/thread state, commands. No UI. |
| `src/main.rs` | `App`, `Route`, `Ui`, spec, scene registry. Keep it small. |
| `src/data.rs` | `Source` (live `Net` or recorded fixtures) and readers over JSON rows (`s`, `ts`, `iso_ms`). |
| `src/theme.rs` | `T` (resolved tokens), Tailwind type scale, radii, spacing, layout constants. |
| `src/tokens.rs` | Generated colour tokens: root scope (`t.c`) and sidebar scope (`t.sb`), dark and light. |
| `src/icons.rs` | Generated: every lucide-react icon apps/web imports, as `icons::NAME`. |
| `src/kit/` | Ports of `apps/web/src/components/ui` primitives (button, badge, menu, dialog, ...). |
| `src/shell/` | Window frame: sidebar v2, layout, global keys. |
| `src/chat/` | Topbar, timeline, messages, work log, composer, home/draft screen. |
| `src/panel/` | Right panel: diff, files, terminal, browser preview (`browser.rs`, CDP-driven Chrome). |
| `src/settings/` | Settings routes. |
| `fixtures/` | A recorded server: `shell.json`, `config.json`, `threads/<id>.json` projections. |
| `tools/` | Generators and the comparison harness. |

Each surface module owns a `Ui` struct (its UI state, reached as `app.ui.<module>`), a `view`, and
`pub const SCENES`. Don't edit another surface's module; if you need something shared, put it in
`kit/` or ask.

## Reading state

Rows are the server's JSON, unchanged (`packages/contracts/src/orchestrationV2.ts`), so field names
match the web client. Use `app.state()`: `.projects`, `.threads`, `.archived`, `.config`,
`.thread_views[thread_id].projection`. Writes go through `app.src.net()` (`dispatch`, `request`);
under fixtures `net()` is `None`, so a write there is a no-op.

## Matching the web

- **Colours**: never hand-pick. `text-muted-foreground` → `t.c.muted_foreground` (inside the sidebar,
  `t.sb.*`; the sidebar re-scopes tokens). `contrast_*` tokens are what `text-foreground`,
  `border-border` and friends resolve to (`--color-foreground: var(--contrast-foreground)`).
  `/75` opacity → `.alpha_mul(0.75)` (`theme::AlphaMul`). Tailwind palette: `theme::tw::EMERALD_500`.
- **Type**: `text-sm` → `theme::TEXT_SM` (14/20), `font-medium` → `theme::weight(.., MEDIUM)`;
  `font-mono` → `theme::mono(..)`. Sans is Inter on Linux and San Francisco on macOS, the same as the
  CSS stack resolves.
- **Sizes**: Tailwind spacing is 4px per unit (`theme::sp`). `size-4` = 16, `h-8.5` = 34, `rounded-md` = 6.
- **Icons**: `icons::SQUARE_PEN` etc., stroke 2 on a 24 grid, sized like the web (`size-4` = 16).
  Brand/provider SVGs that aren't lucide: add them in your module from the web source's SVG.
- **Reference geometry**: every capture has a `.ref.json` listing each painted element's box, colour,
  font, radius, border and padding. Read numbers from there before guessing.

## Build, render, compare

Never run cargo on the dev box. CI builds; Modal renders, on demand, and exits when done.

```sh
# commit, then push HEAD and build it on GitHub Actions (rune-desktop-linux.yml: build + tests);
# downloads the bundle (binary + fixtures + tools). A failed build prints the compiler errors.
tools/ci-build.sh OUT_DIR                 # -> OUT_DIR/bundle.tgz, ~5-8 min
# render scenes headless on an ephemeral Modal container (~1 min for a batch; batch your scenes)
modal run tools/modal_render.py --bundle OUT_DIR/bundle.tgz --scenes home,thread-showcase \
    --themes dark,light --args "--chrome" --out OUT_DIR/shots
# compare against a reference: side-by-side + heatmap + mismatch % (Pillow lives in modal-venv)
/home/ubuntu/modal-venv/bin/python tools/compare.py REF.png RUNE.png OUT.png [x0,y0,x1,y1]
```

Each push to a `feat/rune-*` branch builds; other branch names are pushed to `feat/rune-ci/<branch>`.
A commit must touch `apps/desktop-rune/` to trigger the build. `--tree` dumps (`<scene>.tree.json`
next to each PNG) give Rune's element boxes for geometry checks. Other Rune flags via `--args`:
`--chrome` draws macOS traffic lights; `--script /work/files/x.json` replays input (upload the file
with `--files x.json`); `--at SECONDS` picks the time.

References (macOS desktop layout, 1440x900, DPR 1, `dNN-*.ref.png` + `.ref.json` boxes) are in
the lead's scratchpad `refdesk/shots/`; there is no live server or web app to capture new ones.

## Rune

Source is at `~/.cargo/git/checkouts/rune-*/47cb857/` on the VM (`crates/rune`, `crates/rune-kit`,
README and docs). Read the README's sections on elements, text, scrolling, overlays and key routing
before writing views. `rune_kit` has menus, popovers, dialogs, selects, switches, toasts, tooltips,
markdown and syntax highlighting; restyle them to the web's look rather than shipping their defaults.
