# Plan: Native Kotlin Android Client with Material 3 Expressive

## Goal

Build a standalone, native Android client in `apps/kotlin-android` using Kotlin and Jetpack Compose. It must:

- match the user-visible behavior of the React Native client in `apps/mobile`;
- include every applicable capability introduced by [pingdotgg/t3code#5178](https://github.com/pingdotgg/t3code/pull/5178);
- use Android-native interaction patterns and Material 3 Expressive instead of reproducing iOS chrome;
- make Material 3 Expressive the mandatory product design language on every user-visible screen, using the newest applicable expressive components, shape treatments, size hierarchy, typography, color, and motion rather than merely placing existing layouts inside an expressive theme;
- speak the existing T3 HTTP and Effect RPC WebSocket contracts directly, with no JavaScript runtime;
- remain remote-ready across direct LAN/tailnet, relay, and T3 Connect connections;
- ship signed Android APKs only, with production application ID `club.touchtech.s5code.kotlin`;
- stay in a draft PR until all release gates in this plan pass.

The deduplicated page and feature checklist is maintained in `.plans/24-kotlin-android-parity-tracker.md`.

## Sources analyzed

### React Native client

The current `apps/mobile` implementation is the primary behavior reference because it is already integrated with the current contracts and includes Android-specific behavior. Important reference areas are:

- navigation and deep links: `apps/mobile/src/Stack.tsx`;
- connection/runtime/persistence: `apps/mobile/src/connection`, `apps/mobile/src/persistence`, and `apps/mobile/src/state`;
- user features: `apps/mobile/src/features`;
- Android native controls, FCM, terminal, diff, and composer code: `apps/mobile/modules/*/android`;
- build identity and platform capabilities: `apps/mobile/app.config.ts` and `apps/mobile/eas.json`;
- preview/release automation: `.github/workflows/mobile-eas-preview.yml` and `.github/workflows/mobile-eas-production.yml`.

### Native SwiftUI client in PR #5178

PR #5178 is not in this worktree, so its branch, file list, README, and implementation summary were inspected through GitHub. It adds a native protocol client, secure persistence, direct pairing, optional T3 Connect with DPoP, merged multi-environment workspace, thread and composer flows, files, source control, review, pull requests, terminal, usage, notifications, share extension, widgets/Live Activities, shortcuts, deep links, and extensive native tests.

The Kotlin client must meet the union of the React Native and SwiftUI behavior. When they differ:

1. current server contracts and current React Native behavior win;
2. the SwiftUI app supplies native architecture, resilience, and performance requirements;
3. Android conventions win for presentation and system integration.

### Material 3 Expressive references and authority

Jetpack Compose Material 3 is the implementation path. It is the official Android implementation of Material You and Material 3 Expressive, supports Android 16's visual system, and exposes the theme, component, shape, and motion APIs needed here without a translation layer.

Use references in this order:

1. [Start building with Material 3 Expressive](https://m3.material.io/blog/building-with-m3-expressive) — product intent and the combined use of size, shape, color, containment, typography, and motion to establish hierarchy and improve usability.
2. [Material Design for Android](https://developer.android.com/design/ui/mobile/guides/components/material-overview) and [Material Design 3 in Compose](https://developer.android.com/develop/ui/compose/designsystems/material3) — normative Android implementation and accessibility guidance.
3. Current `androidx.compose.material3` API docs for [`MaterialExpressiveTheme`](https://developer.android.com/reference/kotlin/androidx/compose/material3/MaterialExpressiveTheme), [`MaterialShapes`](https://developer.android.com/reference/kotlin/androidx/compose/material3/MaterialShapes), [`Button`](https://developer.android.com/reference/kotlin/androidx/compose/material3/Button), [`ButtonGroup`](https://developer.android.com/reference/kotlin/androidx/compose/material3/ButtonGroup), [`SplitButton`](https://developer.android.com/reference/kotlin/androidx/compose/material3/SplitButton), [`FloatingActionButtonMenu`](https://developer.android.com/reference/kotlin/androidx/compose/material3/FloatingActionButtonMenu), [`HorizontalFloatingToolbar`](https://developer.android.com/reference/kotlin/androidx/compose/material3/HorizontalFloatingToolbar), and [`LoadingIndicator`](https://developer.android.com/reference/kotlin/androidx/compose/material3/LoadingIndicator) — source of truth for APIs available in the pinned Compose version.
4. [albermonte/android-skills: material-3-expressive](https://www.skills.sh/albermonte/android-skills/material-3-expressive) — Android-specific review checklist for component tokens, devices/window classes, hero moments, reduced motion, contrast, and touch targets.
5. [shelbeely Material Design 3 guide](https://www.skills.sh/shelbeely/shelbeely-agent-skills/material-design-3-guide) — secondary seven-pillar completeness check covering color, motion, type, shape, layout, components, and icons.

The supplied Smithery and ModelScope skills are Tailwind/web-oriented duplicates, so they are not implementation authorities for this native Compose client. They may inform general visual review but must not override official Material or Android guidance.

## Product and engineering decisions

1. **Native only.** Kotlin, coroutines, Flow, Jetpack Compose, and AndroidX; no React Native, WebView app shell, or embedded JS engine.
2. **Compose-first UI.** Jetpack Compose Material 3 is the default and best path for this client because the expressive theme, component sizes, stateful shape morphing, adaptive layout, and motion APIs are native to Compose. Views are allowed only at hard native boundaries such as the Ghostty terminal or a proven high-performance diff canvas; every View boundary must be hosted in an expressive Compose container and inherit the same color, shape, typography, and accessibility semantics.
3. **One app module initially.** Start with `:app` and package-by-feature organization. Split Gradle modules only after measured build or ownership pressure; do not pre-build a large clean-architecture graph.
4. **Thin UI, explicit adapter boundary.** Protocol decoding, transport, authentication, and provider-shaped compatibility stay outside composables. Screen state is immutable and exposed as `StateFlow`.
5. **Offline-tolerant reads and durable writes.** Persist last-known environment/thread state, drafts, and an ordered outbox. Reconnect must not duplicate or reorder turns.
6. **Wire compatibility is tested, not assumed.** Kotlin DTOs use `kotlinx.serialization`; TypeScript-generated fixtures exercise the Kotlin decoders in CI. Unknown optional fields and enum values must degrade safely where the contract permits.
7. **One production identity.** Release/alpha APKs use exactly `club.touchtech.s5code.kotlin`. Debug builds may use `.debug` through `applicationIdSuffix` so they can be installed beside alpha builds. Do not use a different package for PR previews that are intended to update the shared preview installation.
8. **Shared EAS project, distinct Android package.** The Kotlin app uses the existing mobile EAS project ID and owner, but declares `club.touchtech.s5code.kotlin` as its Android package. EAS-managed signing credentials must be explicitly configured and verified for this package rather than assuming the React Native package's credential mapping applies.
9. **APK-only distribution initially.** No iOS artifacts and no Play submission in this scope. Preview artifacts come from GitHub Actions; alpha artifacts are attached to a GitHub prerelease.
10. **Draft until shippable.** The implementation PR is opened as draft and remains draft until the parity tracker and release gates are complete. Removing draft status requires explicit maintainer approval.
11. **Expressive by construction, not by exception.** New screens must be assembled from the shared expressive design layer. A baseline M3 component, custom shape, compact primary action, or bespoke animation is allowed only when the expressive API cannot satisfy the use case, and the parity tracker records the reason. Visual parity with the React Native or SwiftUI client never justifies carrying over non-expressive chrome.

## Proposed project layout

```text
apps/kotlin-android/
  README.md
  settings.gradle.kts
  build.gradle.kts
  gradle.properties
  gradle/libs.versions.toml
  app/
    build.gradle.kts
    src/main/
      AndroidManifest.xml
      java/club/touchtech/s5code/kotlin/
        S5CodeApplication.kt
        MainActivity.kt
        app/                 # root composition, lifecycle, routing
        design/              # mandatory Material 3 Expressive system
          theme/             # color, type, shape, motion, contrast
          component/         # expressive wrappers and semantic defaults
          catalog/           # previews for every adopted component/shape/state
        data/                # stores, database, secure storage, repositories
        transport/           # HTTP, Effect RPC WebSocket, reconnect/session logic
        auth/                # direct tokens, Clerk/T3 Connect, DPoP
        model/               # Kotlin wire and presentation models
        feature/
          onboarding/
          connections/
          home/
          newtask/
          thread/
          files/
          review/
          git/
          pullrequests/
          terminal/
          usage/
          archive/
          settings/
        platform/            # FCM, notifications, live updates, share, widget, shortcuts
      res/
    src/test/
    src/androidTest/
  eas.json
  app.config.json
  Scripts/
    ci-test.sh
scripts/
  generate-kotlin-wire-fixtures.ts
```

Use a version catalog and pin all non-BOM dependencies exactly. Record why any alpha dependency is required. Material Expressive APIs that remain experimental must be isolated behind the design-system package so upgrades do not spread annotations throughout feature code.

## Requested parity verification (2026-08-30)

The release parity set was re-traced from RN contracts and rendering through the
Kotlin projection, UI, commands, persistence, and focused tests. The remaining
gaps were closed: Home now merges debounced, environment-scoped
`orchestration.searchThreads` body matches with local/offline shell matching;
structured input preserves and submits every question; cache clearing uses the
central confirmation host; web/project/action work reports through top-edge or
app-wide progress; transient failures use the global banner; provider marks use
the exact RN path geometry; and Git can run the contract-native `create_pr`
action. The syntax pass now replaces keyword-profile tokenization with pinned
KotlinTextMate 0.2.0 plus 84 APK-local TextMate grammars. Source files, review
diffs, and fenced blocks resolve 60 directly selectable RN-compatible languages,
carry grammar state across lines, retain only eight compiled and 24 raw grammars,
and fail closed to exact plaintext without JavaScript or runtime downloads. Fresh
gates passed: `:app:lintDebug` (zero errors or warnings; four
informational hints), `:app:testDebugUnitTest` (418 tests, zero failures, one
live-server test skipped without credentials), and `:app:assembleDebug`. The
rebuilt APK is 55,483,404 bytes with SHA-256
`b9a2372c11032e47c92bd7637530c38ae26071b744d2fa57a7ccf87ead2cdcf8`; package,
version, SDK levels, and signature are recorded in the release notes after
independent Build Tools inspection. Device/emulator UI and install/update
verification were not run because this host has the Android SDK but no emulator
binary, AVD, or attached device. All visible in-flight indicators are now confined
to the design layer and use pinned Material 3 Expressive primitives: contained or
inline morphing loaders, wavy top-edge progress, and expressive pull-to-refresh;
a source audit test prevents baseline circular/custom infinite loaders returning.

## Runtime architecture

### Transport and sessions

Implement the existing direct-server protocol rather than inventing a mobile API:

- HTTP client for pairing, assets, workspace files/previews, and other HTTP endpoints;
- Effect RPC WebSocket framing compatible with `packages/client-runtime/src/rpc` and `packages/contracts/src/rpc.ts`;
- one supervised session per environment with explicit `Disconnected`, `Connecting`, `Connected`, `Recovering`, and `AuthRequired` states;
- exponential reconnect with jitter, lifecycle-driven foreground socket replacement, connectivity callbacks, keepalive, request cancellation, and stale-connection generation guards;
- independent environment failures so one unavailable machine does not block the merged home;
- incremental event application rather than full-list replacement on every message;
- scoped subscriptions and bounded caches to avoid keeping unused clients alive.

### Contract compatibility

Do not try to import TypeScript into Kotlin. Instead:

1. define explicit serializable Kotlin wire DTOs at the transport boundary;
2. add `scripts/generate-kotlin-wire-fixtures.ts`, modeled on PR #5178's Swift fixture generator;
3. generate representative success, optional-field, unknown-field, and malformed fixtures from `packages/contracts`;
4. decode them in JVM tests and fail CI when generated fixtures drift;
5. centralize wire-to-domain mapping so UI models never contain `JsonElement` or transport nullability;
6. document unsupported provider capabilities and render them as unavailable rather than guessing.

### Persistence and security

- Room for environments, cached entities, thread pages, drafts, outbox entries, recents, and user preferences.
- Android Keystore-backed encrypted storage for access tokens, relay credentials, DPoP keys, and account secrets. Never put credentials in Room, logs, intents, or saved-state bundles.
- DataStore for small non-secret preferences.
- Atomic migrations with tests from every released schema.
- Bounded transcript, markdown, image, terminal, favicon, and diff caches with memory-pressure handling.
- Durable outbox entries carry stable client IDs, dependencies, and delivery state. Drain serially per thread and acknowledge from authoritative receipts/events.

**Deviation, as built:** preferences, drafts, and recents persist through
`data/ClientStateStore.kt` (SharedPreferences holding one JSON document), not
Room. Room buys migrations, queries, and partial writes; this state is a few KB
read once at launch and rewritten whole, so none of those apply, and a schema plus
DAO plus migration test for it would be machinery without a constraint behind it.
Enums persist by name rather than ordinal precisely because that is where this
shape can break — reordering an enum must not silently reinterpret a saved value.
Room still earns its place when cached transcripts, thread pages, and the outbox
arrive, which are the parts that need partial reads and real migrations.

**Deviation, as built:** composer attachment copies (pruned at 24 hours), Coil's
64 MB image disk cache, and per-environment last-known workspace snapshots now
live on disk. `WorkspaceSnapshotStore` keeps one replace-whole shell document plus
one document per opened transcript under `filesDir/workspace-snapshots`, so Android
cannot evict offline chats as temporary data. Cold start races those reads against
live subscriptions: cached rows/transcripts render immediately, and a faster network
snapshot wins through compare-and-set. Every reduced shell/thread event is written
atomically; corrupt records fail open and are removed. Settings → Client storage
measures and clears all three real cache categories. Markdown blocks, highlighted
lines, diffs, and terminal frames remain composition-scoped by content fingerprint.

### Direct pairing and T3 Connect

Support both connection models:

- paste/type a pairing URL;
- scan a QR code with CameraX/ML Kit or the smallest maintained scanner dependency;
- validate schemes/hosts and exchange one-time pairing credentials;
- local-network diagnostics and actionable cleartext/LAN errors;
- saved environment management and per-device health;
- Clerk sign-in using a maintained native Android SDK or standards-based browser flow;
- DPoP key creation, JWK thumbprints, signed proofs, nonce/retry behavior, relay discovery, managed-environment token exchange, token recovery, and sign-out cleanup matching the current relay contract.

Authentication choices must be proven against the existing S5 Clerk configuration before feature UI is built around them.

**As built.** Every bullet above except local-network diagnostics is implemented:
QR scanning is CameraX plus ML Kit's bundled model (`platform/QrScanner.kt`), and
S5 Connect runs a real DPoP flow (`cloud/Dpop.kt`, `cloud/RelayClient.kt`,
`cloud/RelayEnvironmentAuthorizer.kt`). Two details are worth recording because
they are invisible until the relay rejects a request: the ES256 signature must be
converted DER→JOSE, and the token exchange's `resource` must be the relay origin
with no trailing slash. Both are pinned by tests. Nonce retry is not implemented
— the relay does not currently issue `DPoP-Nonce` — so the client handles a
rejected access token by refreshing once instead.

## Compose and mandatory Material 3 Expressive design

### Non-negotiable design contract

Material 3 Expressive is a release requirement, not optional polish. Every screen and state must deliberately use all applicable pillars: color, typography, shape, size/layout, containment, components, icons, and motion. Reviewers should reject a screen that only inherits `MaterialExpressiveTheme` while retaining generic small controls, uniform cards, or baseline component choices.

Create `S5ExpressiveTheme` around `MaterialExpressiveTheme` and a small semantic component layer. Experimental API opt-ins remain isolated under `design/`; features consume stable S5 wrappers. Pin Compose Material 3 exactly, review every release for newly available expressive APIs, and maintain an adoption ledger in the parity tracker. When an API changes, update the wrapper rather than duplicating compatibility code across features.

### Foundations

- **Color:** light, dark, and system modes; dynamic color where supported; expressive branded fallbacks; deliberate primary/secondary/tertiary and surface-container contrast; fixed accents only where supported and useful. Color must create emphasis and state, not become decoration or the only state signal.
- **Typography:** use the expressive/emphasized type roles available in the pinned library for hero titles, primary values, and key status; preserve highly readable body/chat text. Monospaced typography is isolated to code, diffs, paths, logs, and terminal content. Support user scaling without clipping.
- **Shape:** define the complete expanded `Shapes` scale, including increased sizes. Expose every `MaterialShapes` shape available in the pinned version through a named design registry and preview it in the catalog. Use iconic shapes for avatars, status, hero art, selected states, and focused moments; do not scatter all shapes randomly through dense technical lists. Interactive components use state-aware shape morphing when the API supports it.
- **Motion:** use `MotionScheme.expressive()` and component-native spring/shape transitions for prominent state changes, navigation, container transformations, selection, and hero interactions. Frequent transcript/list updates use restrained motion. Reduced-motion mode removes nonessential transforms and morphs while preserving immediate state feedback.
- **Icons:** use Material Symbols/Compose Material icons consistently, including filled/outlined state changes where useful. Every ambiguous icon-only control gets a localized content description and tooltip.
- **Containment and depth:** use the surface-container hierarchy and expressive shape/size contrast to group related information. Avoid a wall of identical cards, excessive elevation, background blur without a supported native API, or color/shape variation without hierarchy.

### Expressive component adoption matrix

Use the newest expressive component that semantically fits. All available states, sizes, light/dark variants, large-font behavior, and reduced-motion behavior must appear in the Compose design catalog and screenshot suite.

- **Actions:** expressive `Button`, `FilledTonalButton`, `OutlinedButton`, `TextButton`, icon-button, and toggle-button overloads with `ButtonShapes`/state morphing. Use `SplitButton` for a primary action plus closely related alternatives, and `ButtonGroup` for related actions or connected selection. Do not recreate these with `Row`, `Surface`, or custom pointer handling.
- **Action sizing:** primary/hero actions use `ButtonDefaults.LargeContainerHeight` or `ExtraLargeContainerHeight`; normal prominent actions use at least `MediumContainerHeight`; small is reserved for genuinely secondary, space-constrained actions; extra-small is prohibited for primary flows. Use the matching `contentPaddingFor`, `iconSizeFor`, `iconSpacingFor`, `textStyleFor`, and shape helpers rather than hard-coded dimensions. Every target remains at least 48×48dp, and larger where the expressive spec provides it.
- **FABs and menus:** use medium/large or extended FABs for the single highest-priority screen action. Use `ToggleFloatingActionButton` + `FloatingActionButtonMenu` when that hero action expands into a short action family. Never add multiple competing FABs.
- **App bars and toolbars:** use `MediumFlexibleTopAppBar`/`LargeFlexibleTopAppBar` where title hierarchy benefits from expansion. Put contextual page actions in `HorizontalFloatingToolbar`/`VerticalFloatingToolbar` or `FlexibleBottomAppBar`, including the paired vibrant/standard FAB variants, rather than crowding navigation chrome. Respect built-in TalkBack behavior that keeps controls available.
- **Selection:** prefer expressive toggle buttons and connected button groups over legacy segmented controls. Use filter/input chips only where their semantics remain correct.
- **Loading and progress:** use `LoadingIndicator` for bounded, visible loading moments and its determinate form when progress is known. Avoid an always-running expressive animation in rows, backgrounds, or idle screens; use static/skeleton/standard progress treatment for repeated dense loading.
- **Menus and lists:** adopt expressive menu and list-item variants available in the pinned version, including grouped/selectable/toggleable semantics and overflow behavior, instead of custom popup/card implementations.
- **Containers and navigation:** use current M3 cards, sheets, dialogs, search, navigation bar/rail/drawer, carousel, and adaptive panes with the expressive theme and shape scale. Prefer official components over custom facsimiles even when the older client looks different.
- **New APIs:** at each Compose Material 3 upgrade, inventory the release notes and API surface for newly stabilized or introduced expressive components. Every applicable component must be adopted or have a maintainer-approved `N/A` reason in the tracker; “existing wrapper already works” is not sufficient.

“All shapes/components” means complete catalog/token coverage and use in every semantically applicable product flow—not placing every decorative shape or component on every screen. Hierarchy, usability, accessibility, and dense coding content remain the constraints that make expression intentional.

### Layout mapping

Preserve information architecture, not iOS chrome:

- phones: flexible top app bars, modal bottom sheets, full-screen task/thread flows, a large or extended FAB/FAB menu for the hero action, and floating/flexible bottom toolbars for contextual actions;
- medium widths: navigation rail plus content where useful, with medium FABs and horizontal or vertical floating toolbars selected by available space;
- expanded/tablet/foldable widths: list-detail workspace with resizable or stable panes;
- use `WindowSizeClass` and adaptive navigation; preserve selection across resizing;
- predictive back previews the actual destination and works through sheets, nested screens, and detail panes;
- keyboard, mouse, stylus, and hardware shortcuts are first-class on tablets/Chromebooks.

### Performance rules

Expression cannot compromise the repository's performance standard. Prefer component-native transitions, cap concurrent shape morphs, and animate only visible/active elements. Do not animate a whole lazy list because one row changes.

- `LazyColumn`/`LazyVerticalGrid` keys must be stable; use paging for long transcripts and large homes.
- Collect only the state a composable needs and use immutable models.
- Throttle streaming markdown presentation without delaying final content.
- Cache parsed completed markdown and syntax-highlight results by content fingerprint.
- Render diffs and terminal frames incrementally; cap terminal scrollback/buffers.
- Avoid infinite decorative animations and high-frequency timers. Only active rows may update elapsed time frequently.
- Establish macrobenchmarks for cold start, merged-home scroll, long-thread open/scroll, streaming response, and large diff review.

## Delivery phases

Each phase ends with focused tests and a usable vertical slice. Update `.plans/24-kotlin-android-parity-tracker.md` in the same PR as implementation.

### Progress against these phases

Where the app actually stands, so the phase list below reads as a plan rather than
a claim:

- **Phases 0-2 (baseline, scaffold, direct connection):** done. The expressive
  design layer, adaptive navigation, secure storage, pairing by URL *and* QR, the
  RPC transport, and the reconnect state machine are all live against a real
  server.
- **Phase 3 (multi-environment home and lifecycle):** partial. Screens read live
  data through `LiveWorkspaceGateway`; concurrent sessions and collision-safe
  row identity are live. Home now turns approval/input shell flags into
  actionable cards by subscribing only those attention threads, exposes title
  regeneration only on capable environments with server-authoritative in-flight
  state, highlights visible search matches, and renders the Thread List V2
  project/provider/branch/status metadata without an infinite row animation.
  Per-environment shell snapshots now restore cached last-known rows at cold start;
  explicit stale badges and the large-list benchmark are not done.
- **Phase 4 (new task and thread core):** partial. Pickers, transcript, streaming
  Markdown, tool rows, work-group and whole-turn folding, live-follow with a jump
  back to the tail, model search, attachments, draft persistence with the draft
  visible on the home row, and deep-link route resolution are in. Existing-thread
  model, runtime, and option changes are staged in the environment-scoped composer
  draft, restricted to the thread's current provider instance, persisted, and
  synchronized immediately before the next turn; that turn carries the identical
  settings snapshot. Opened transcripts restore from disk during cold start. The
  composer now uses RN's tiered fuzzy ranking for provider slash commands and
  server-backed `@` paths, reports loading/syncing/reconnect failures inline, and
  opens full-resolution draft-image previews. Existing-thread and new-task sends
  enter an environment-scoped durable outbox first; stable command/message ids,
  app-private attachment copies, FIFO idle-thread dispatch, transient exponential
  backoff, process-death restore, acknowledgement removal, and queue-count UI are
  wired end to end. New-task navigation is allowed to race bootstrap safely: the
  temporary detail-stream not-found waits on outbox/shell state instead of failing
  the app scope. If Android dies after server acceptance but before local cleanup,
  a matching restored shell row or the exact typed same-ID duplicate invariant is
  treated as the missing acknowledgement rather than replaying creation. Live
  Update arming is best-effort and cannot unwind durable acceptance. Focused
  `ThreadOutboxTest` cases pin the positive recovery paths and reject unrelated
  IDs, failure kinds, and exception types. The prompt stash was built and then removed: on a phone each
  thread already owns a persistent draft, so a second holding queue was a second
  place for the same prompt to sit. Transcript pagination is not done.
- **Phase 5 (workspace tools):** partial. The flat recursive workspace index is
  reconstructed into a hierarchical file tree with ranked in-tree path/name
  filtering, matching-ancestor reveal, Pierre extension icons, and bounded
  prewarming. Source and signed-URL reads deduplicate in flight; source prewarm is
  a 24-entry/2 MiB LRU with RN's 256 KiB per-file limit. Coil image previews cap
  decoded memory at 24 MB and disk at 64 MB and show decoded resolution/response
  bytes. The signed-asset WebView exposes URL/back/forward/reload controls while
  preserving same-origin containment and canceling every SSL error. Source
  highlighting uses pinned KotlinTextMate 0.2.0 with 84 APK-local TextMate
  grammars: 60 are directly selectable through the RN Shiki language/alias
  catalog and the remainder resolve embedded includes. Source, review, and fence
  batches preserve multiline grammar state, keep only eight compiled and 24
  parsed grammars in bounded LRUs, perform no JavaScript execution or grammar
  download, and preserve exact plaintext on unsupported labels or engine failure.
  Markdown renders GFM-style aligned
  pipe tables, safe external links, and Pierre-icon workspace file links that
  stay inside the active project/worktree and route to the in-app viewer.
  Transcript, Markdown, and composer images share a fullscreen pinch/pan/double-tap
  lightbox, including signed transcript attachments. Working-tree review now
  supports tap/long-press line and range comments, RN-compatible structured
  review-comment round trips, restrained intra-line word diffs, direct file
  navigation, and a short-lived parsed diff cache prewarmed from Git overview.
  Usage has its window control and per-window series. The terminal is live over
  `terminal.attach` and now renders through the same upstream Ghostty VT engine as
  React Native, exposed directly to Kotlin through CMake/JNI and a Canvas View.
  Alternate-screen TUIs, Unicode/color/cursor state, bounded scrollback, touch
  selection, Meslo Nerd Font glyphs, Pierre light/dark themes, server Clear/Restart,
  Copy all, software keys, and terminal-mode-aware hardware keys are wired. Multiple
  terminal sessions/tabs remain pending. Review, git beyond the confirmations, and
  PRs are not there.
- **Phase 6 (T3 Connect and platform integrations):** partial. Clerk sign-in,
  DPoP, the relay token exchange, managed environment discovery and connection,
  deep links, the sharesheet, launcher shortcuts, global physical-keyboard
  commands, and the interaction-parity pass are implemented. Expanded windows
  now use a real three-column workspace: the merged Home/thread list remains on
  the left, chat remains live in the center, and files, terminal, Git, review,
  and their child routes navigate in an independent trailing inspector. The
  inspector has a 16dp drag target, 260–480dp bounds, custom accessibility resize
  actions, saved preferred width, and a 560dp chat floor; it falls back to center
  navigation when the window cannot retain that floor. Cmd/Ctrl+N starts a task,
  Cmd/Ctrl+K reveals and focuses Home search, Cmd/Ctrl+Enter submits the focused
  thread/new-task composer while bare Enter remains a newline, and Escape clears
  search/focus, closes the inspector, or navigates back. Ghostty consumes its own
  keys first (`app/{S5App,NavGraph,HardwareShortcuts,WorkspacePanes,WorkspaceInspectorHost}.kt`,
  `design/component/WorkspacePaneDivider.kt`, `feature/home/HomeScreen.kt`,
  `design/component/ComposerField.kt`). The interaction pass adds app-scoped
  operation progress/error presentation,
  centralized destructive confirmations, Android haptics for copy/refresh/swipe and
  operation results, resistant swipe detents, top-edge loading, stable lazy-item
  enter/exit/reorder motion, bounded dynamic project favicons, and branded provider
  marks (`design/component/{ActionProgressOverlay,GlobalErrorBanner,ConfirmDialog,PullToRefresh,LoadingStrip,ProjectIcon,Provider}.kt`).
  Git and rewind operations publish typed phase/result state through `AppStore`, so
  Their banner survives navigation and can expose the contract's PR URL. Returning
  an existing process from Android background now invalidates every possibly
  half-open WebSocket on `ON_START`, waits for each old supervisor's cleanup, and
  then authorizes a fresh socket so old `finally` blocks cannot erase replacement
  connections. Session phases publish independently of shell frames, and shell
  relative-time projections refresh from one shared minute clock. Home working
  rows retain the active turn start and tick only while that lazy row is visible,
  once per second, so “Working · 12s” no longer freezes while avoiding a global
  high-frequency timer (`app/{S5App,ForegroundRefreshGate}.kt`,
  `transport/EnvironmentSession.kt`, `data/{LiveWorkspaceGateway,Projection}.kt`,
  `feature/home/ThreadRow.kt`). Firebase
  Messaging integration, token rotation persistence, serialized Relay
  `mobile:registration`, alert deep links, and API 36 promoted ongoing Live Updates
  are now implemented under `platform/notifications`. Runtime delivery remains
  configuration-gated: `apps/mobile/google-services.json` has no Firebase client
  for `club.touchtech.s5code.kotlin`, so source/prerelease builds without injected
  public Firebase options show Unconfigured instead of claiming registration.
  Device/emulator verification and the Glance widget remain.
- **Phases 7-8 (hardening, release):** not started, except that a debug APK has
  been hand-published as a GitHub prerelease on the fork for install testing:
  [`kotlin-android-v0.1.0-alpha.1-debug`](https://github.com/SparshKaushik/s5code/releases/tag/kotlin-android-v0.1.0-alpha.1-debug).
  That is a test build, not the signed alpha Phase 8 describes.

The two blockers above are the only items in this plan that cannot be moved
without something outside the repo. Everything else is ordinary remaining work.

### One lesson worth carrying forward

Three decode bugs reached a device while every JVM test passed: a DPoP header
missing two fixed JWK members, `platform` typed as a string where the contract has
a struct, and `/api/auth/session` sent as a `POST` where the contract defines a
`GET`. They shared a cause — the fixtures were written from the same assumption as
the code, so they agreed with each other and not with the server. Each surfaced as
a confident, misleading message ("the relay rejected this device proof key", "that
address answered, but not like an S5 Code server") that pointed at the wrong
layer.

The fix is `apps/kotlin-android/.../transport/LiveServerContractTest.kt`: it runs
against a real server seeded from a copy of real data and decodes every payload
this client reads. Hand-written DTOs are a translation of a contract in another
language, and the only honest check on a translation is the original. Prefer this
over the fixture generator Phase 0 proposed — a generator only proves the decoder
matches the generator.

### A second lesson: mirror the source of truth, do not summarise it

A later on-device pass found seven behavioural mismatches, and five of them came
from the same habit: a plausible Kotlin-side simplification of something RN already
decides carefully.

- `ProviderDriver` was a closed enum of five drivers. `ProviderDriverKind` is an
  open branded slug, so pi rendered as Codex and any fork's driver would too. The
  cheap fix — add a `Pi` entry — would have left the next one broken; the client
  now carries the slug and routes on `instanceId`.
- One `ThreadStatus` enum served as both the status pill and the home-list
  partition, so a settled thread whose session had errored sorted into the active
  list. RN keeps those two derivations separate on purpose.
- The feed sorted on a synthetic sequence that outranked timestamps, and tool
  lifecycle rows were never collapsed, so a tool call could render below the last
  message.
- The composer swapped `TextFieldLineLimits` on focus, which cannot work: Android
  negotiates IME options once per input session, so the keyboard kept its Done key
  and Enter never inserted a newline.
- The usage chart was hardcoded to 30 days with no window control at all.

A following pass found the terminal in the same shape, and it is the clearest
example yet. The screen called `terminal.open` once as a plain read and rendered
`history` as text. Each piece is defensible in isolation and the whole thing could
never work: the echo of what you type only arrives as a later `attach` frame, so
nothing you typed appeared; `terminal.write` declares no success value, so decoding
Effect's valueless exit into a struct made every keystroke report a failure; and
the stream is real PTY bytes, so the prompt read as `[?2004h`. RN's
`packages/client-runtime/src/state/terminal.ts` had all three answers already.

When a behaviour exists in `apps/mobile` or `packages/`, port it and cite the
counterpart in a comment, the way `data/Projection.kt` already does. An invented
equivalent looks right in review and disagrees with the other clients on the cases
nobody thought to check.

### A third lesson: port the behaviour, not the surface it lives on

The prompt stash was a faithful port of `apps/web/src/promptStashStore.ts` and was
the wrong feature anyway. Web needs a holding queue because a desktop user has one
composer and many threads behind it; a phone gives each thread its own persistent
draft, so the queue duplicated storage the client already had. What the maintainer
actually wanted from the desktop sidebar was the *indicator* — being able to see
which threads have something unsent — and that turned out to be one line on a home
row over state that was already persisted.

Ask what the web or RN affordance is compensating for before porting it. A feature
that is correct on a 27-inch screen with a sidebar can be redundant on a phone with
a back stack, and shipping it costs a schema field, a sheet, a composer control,
and the reader's attention.

### Phase 0 — Baseline and contract freeze

- Add the parity tracker and assign every row a source reference and test expectation.
- Record supported server API/contract revision and provider matrix.
- Capture representative wire fixtures and anonymized performance datasets.
- Decide minimum SDK from capability requirements; target/compile the repository's Android baseline (currently API 36 for mobile).
- Confirm the existing mobile EAS project is configured for `club.touchtech.s5code.kotlin` and generate/import the package's remote keystore mapping.

**Exit:** fixture generation runs, release identity is reserved, and no open architectural question blocks the first vertical slice.

### Phase 1 — Scaffold and expressive design system

- Create Gradle Kotlin DSL project, Compose app, dependency catalog, lint, detekt, ktlint, unit/instrumentation test setup, baseline profile plumbing, and debug/release build types.
- Implement the complete expressive foundation: `MaterialExpressiveTheme`, dynamic/fallback color, expressive and emphasized typography, expanded shape scale, full `MaterialShapes` registry, `MotionScheme.expressive()`, icon policy, semantic spacing/sizing, and reduced-motion behavior.
- Build shared wrappers and catalog previews for all applicable expressive action sizes and shape states, buttons/icon/toggle buttons, split buttons, connected/standard button groups, flexible app bars, floating toolbars, flexible bottom app bar, FAB/FAB menu, loading indicators, expressive menus/list items, cards, sheets, dialogs, navigation, and adaptive containers available in the pinned Compose version.
- Implement edge-to-edge activity, adaptive root navigation, expressive empty/loading/error states, app-link/custom-scheme routing skeleton, and secure log redaction.
- Add screenshot tests across phone/tablet, light/dark, representative dynamic color, large font, interaction states, and reduced motion. Add a lint/static rule or architecture test preventing direct feature-level use of expressive experimental APIs and hard-coded compact primary-button dimensions.

**Exit:** debug APK launches on phone and tablet configurations; the catalog proves complete available-component and `MaterialShapes` coverage; theme, accessibility, screenshot, and idle-animation checks pass; CI builds unsigned debug artifacts.

### Phase 2 — Direct connection vertical slice

- Implement secure environment storage, pairing URL parsing/exchange, QR scanner, HTTP/WebSocket RPC, reconnect state machine, and one-environment cache.
- Render real projects and threads from a development server.
- Add remove/reconnect and clear error recovery.

**Exit:** a fresh install can pair, reconnect after process death, list real threads, and remove all credentials.

### Phase 3 — Multi-environment home and lifecycle

- Add concurrent environment sessions, merged collision-safe identities, cached last-known rows, health indicators, search/filter/group/sort, thread actions, archive, and adaptive list-detail behavior.
- Apply incremental updates and lifecycle-aware foreground/background activity.

**Exit:** disconnected and connected environments coexist; large-list benchmark and multi-environment tests pass.

### Phase 4 — New task and thread core

- Implement project/environment/branch/provider/model/settings pickers and project add/clone/create flows.
- Build paginated transcript, rich streaming Markdown, tool/work-log rows, images, composer commands/mentions, draft persistence, durable outbox, cancellation, approvals, structured input, and thread lifecycle actions.
- Add notification/deep-link route resolution even before remote push is enabled.

**Exit:** users can create and complete real turns through every provider-supported request type, survive offline/reconnect, and reopen long threads without state corruption.

### Phase 5 — Workspace tools

- Files and previews, hierarchical search/filtering, bounded preload/cache,
  image metadata, source highlighting, image/Markdown/web preview, and fail-closed
  SSL-aware WebView controls.
- Working-tree review with file sections, word diffs, comments, and full-diff hydration.
- Git status/actions, branches, commit, sync, PR metadata/actions.
- Native Ghostty-backed terminal with multiple sessions and keyboard controls. The
  native renderer, themes, Nerd Fonts, actions, and keyboard controls are complete;
  multiple sessions/tabs remain. It reuses the proven `libghostty-vt.so` build
  approach from `apps/mobile/modules/t3-terminal/android`, exposed directly to
  Kotlin rather than through Expo.
- Usage dashboard.

**Exit:** each tool works through local, remote/relay, and tunnel connections where supported; large file/diff/terminal stress tests pass.

### Phase 6 — T3 Connect and Android platform integrations

- Native Clerk/T3 Connect + DPoP and managed relay recovery.
- FCM registration and approval/input/completion/failure notifications are implemented: API 33 permission/channel setup, persisted token rotation, account-scoped Relay upsert/unregister, signature dedupe, standard alert rendering, and cold/warm validated thread navigation. Runtime delivery still requires injected Firebase client options for this package.
- Android 16 Live Updates are implemented with `Notification.ProgressStyle`, promoted-ongoing requests, persisted generation and event ordering, bounded plan progress, dismissal semantics, Relay replay registration, and validated deep links. Older Android versions continue on standard alert notifications; API 36 device verification remains.
- Android Sharesheet receive flow for text, URL, and up to eight images into a durable new-task draft.
- Pinned shortcuts for New Task and recent threads.
- Glance recent-task/agent-activity widget as the Android counterpart to PR #5178's WidgetKit surface.
- Background refresh/reconciliation, haptics, and notification permission/settings UX.

**Exit:** platform entry points work from cold start, process death, and multiple environments; notification and share payload tests pass.

### Phase 7 — Settings, polish, and hardening

**Interaction parity landed early:** top-edge loading and a navigation-independent
operation banner now cover source-control, web-preview, project mutation, and
rewind work; terminal operation states auto-dismiss and emit success/failure
haptics. One app-root confirmation host owns thread deletion, environment removal,
rewind, and individual/all cache clearing. A six-second global error banner
replaces one-off transient failures across Home, Thread, Archive, terminal, project,
cache, and source-control actions. Copy, pull refresh, swipe threshold/commit, and
operation completion use Android haptic types. Lazy rows animate keyed
enter/exit/reorder without continuously repainting idle content. Composer intake
adds ranked fuzzy command/path discovery, inline connection/sync phase pills,
full-resolution attachment preview, and a durable FIFO outbox whose stable ids and
app-private attachment copies survive process death and drain with capped
exponential backoff. Project favicon
resolution is dynamic with Coil disk/network caching plus a bounded decoded LRU,
and provider rows use the exact RN Claude, Cursor, Grok, OpenCode, Codex, and pi
vector geometry with an unknown-driver fallback. Home search now merges local
shell-field matches with `orchestration.searchThreads` message-body snippets and
highlights title/subtitle content in place. Structured input preserves every
question and submits one question-id-keyed answer record. Focused wire, projection,
answer-building, search, and cache-bound tests live alongside
`GitActionProgressWireTest`, `ThreadSearchWireTest`,
`PendingUserInputProjectionTest`, `PendingUserInputAnswersTest`, and
`ProjectFaviconCacheTest`.

- Finish all settings and legal/storage pages.
- Accessibility: TalkBack order/labels/actions, scalable text, contrast, touch targets, reduced motion, switch access, and keyboard navigation.
- Localization-ready strings; no user-facing literals in Kotlin.
- Security review of intent/deep-link validation, logs, token storage, WebView previews, file handling, and DPoP lifecycle.
- Performance and memory profiling using real-data snapshots copied into test fixtures, never the live T3 home.
- Add user docs and internal architecture/release docs.

**Exit:** tracker is complete or has maintainer-approved Android-not-applicable rows with reasons; no critical/high defects remain.

### Phase 8 — Preview and alpha release

- Run signed preview builds on the draft PR through GitHub Actions.
- Complete physical-device matrix and integrated client pass with explicit permission.
- Produce a signed alpha APK, checksum, SBOM/provenance where supported, and release notes.
- Publish an Android-only GitHub prerelease; do not attach React Native, iOS, desktop, or server artifacts to this alpha release.
- Keep the PR draft until post-alpha blockers are fixed and maintainers explicitly approve readiness.

## CI, EAS signing, previews, and alpha releases

### EAS project files

A bare native Android project can still use EAS Build. Add a minimal Expo app config only for EAS metadata; do not add the Expo runtime to the app:

- `app.config.json`: reuse the existing mobile app's slug, owner, and EAS project ID while setting `android.package = club.touchtech.s5code.kotlin`;
- `eas.json`: `preview` and `alpha` profiles, both Android-only, `buildType: apk`, `credentialsSource: remote` explicitly set;
- Gradle release signing remains free of checked-in secrets; EAS injects the remote keystore during build.

Before CI uses `--non-interactive`, run one maintainer-controlled interactive setup against the existing mobile EAS project to register `club.touchtech.s5code.kotlin` and configure its remote Android credentials. Store only `EXPO_TOKEN` and existing project metadata in GitHub/EAS; never commit or print the keystore/passwords.

### Pull-request preview workflow

Add `.github/workflows/kotlin-android-preview.yml`:

- triggers only when `apps/kotlin-android`, relevant contracts, fixture generator, shared platform contracts, or the workflow changes;
- requires the existing mobile continuous-deployment label or a dedicated Kotlin preview label to control cost;
- runs JVM tests, contract fixture check, lint/detekt/ktlint, Compose screenshot tests, and `assembleDebug` first;
- performs `eas build --local --platform android --profile preview --non-interactive` on Ubuntu so signing credentials come from EAS but the APK is retained by GitHub Actions;
- uploads the signed APK with `actions/upload-artifact` and comments one stable PR comment with artifact/run links, application ID, commit SHA, and expiry;
- uses PR-number concurrency with cancellation of superseded builds;
- never uses production data or exposes credentials to forked PRs.

Unlike the React Native client, there is no OTA path for Kotlin bytecode/resources. Every preview revision requires a new APK. Gradle and EAS caches should reduce build time, but correctness must not depend on cache reuse.

### Alpha workflow

Add `.github/workflows/kotlin-android-alpha.yml` with manual dispatch and optional tags such as `kotlin-android-v0.1.0-alpha.1`:

- validate semantic alpha version and monotonically increasing `versionCode`;
- run the same release gates as preview plus release unit/instrumentation tests and APK install smoke test;
- build only Android via the EAS `alpha` profile using remote credentials;
- verify package name and signing certificate fingerprint against an expected GitHub variable;
- upload `s5code-kotlin-<version>.apk` and SHA-256 checksum;
- create a GitHub **prerelease** containing only those Android files and generated release notes;
- no Play submission and no AAB until explicitly added later.

Do not modify the unified `.github/workflows/release.yml` for the alpha experiment. Integration into stable releases is a separate decision after the app is shippable.

## Testing strategy

### Pure/JVM tests

- pairing URL and deep-link validation;
- RPC framing, reconnect generations, cancellation, and protocol errors;
- every generated contract fixture;
- wire-to-domain mapping and unknown optional fields;
- merged environment identities, sorting, grouping, and stale cache behavior;
- outbox dependency ordering, idempotency, retry, and acknowledgement;
- Markdown parsing/model creation, diff parsing, file paths, terminal caps;
- DPoP signing vectors, nonce/retry, token recovery, and secret redaction;
- notification/live-update/share payload validation.

One of these has changed in practice. "Every generated contract fixture" is
replaced by a live decode sweep against a real server
(`transport/LiveServerContractTest.kt`), for the reason recorded under Delivery
phases: a generated fixture proves the Kotlin decoder matches the generator, not
the server, and the three decode bugs this project shipped were all invisible to
fixtures written alongside the code.

### Compose/UI tests

- design-catalog coverage for every expressive component wrapper, every supported button size/state, and every `MaterialShapes` registry entry;
- assert primary actions resolve to large/extra-large sizing, prominent actions to medium or larger sizing, and all targets to at least 48×48dp unless an official component provides equivalent accessible semantics;
- screenshot/golden matrices for light/dark/dynamic color, standard/high contrast where available, font scale, shape states, and reduced motion;
- navigation and reverse actions for every page;
- loading, empty, stale, offline, unauthorized, partial-failure, and retry states;
- phone, foldable, tablet, font-scale, dark/light/dynamic-color, RTL, and reduced-motion variants;
- approvals, multi-select input, composer IME behavior, and predictive back;
- screenshot/golden coverage for design-system primitives and critical pages;
- screen-level expressive audit: each destination documents its hero action, size hierarchy, shape/containment choices, applicable new components, and any approved exception.

### Integration tests

- run against an isolated T3 server/home and wait for typed receipts or authoritative events, never arbitrary sleeps;
- direct LAN, relay, tunnel, reconnect, process death, and multi-device routing;
- all five providers, with unsupported capabilities explicitly asserted;
- FCM/deep-link cold start and share cold start;
- Room migration and restored encrypted credentials.

### Performance tests

- Macrobenchmark + Baseline Profile for cold/warm startup, home scroll, long-thread navigation, streaming, large review, and terminal interaction;
- memory ceilings for cached transcript/markdown/images/diffs/terminal;
- no continuously repainting idle animation and no per-second global list invalidation.

## Provider and connection matrix

Every provider-shaped feature must be recorded as supported, unsupported, or degraded for Codex, Claude, Cursor, Grok, and OpenCode. At minimum verify:

- model/provider selection and catalog evolution;
- plan mode and canonical plan progress;
- approvals and structured user input;
- tools/work logs and subagents;
- images and attachments;
- cancellation, rewind, and title generation;
- terminal, files, review, source control, and PR operations.

Run the same user flow over direct local, tailnet/tunnel, and managed relay connections. Transport differences may affect availability, but must not create silent UI divergence.

## Documentation deliverables

- `apps/kotlin-android/README.md`: setup, local build, pairing, EAS project, credential bootstrap, test commands, and limitations.
- `docs/user/`: installation, pairing/T3 Connect, Android notifications/live updates, sharing, widgets, and alpha caveats.
- `docs/internals/`: Kotlin architecture, RPC compatibility, persistence/outbox, Material Expressive system, and contract fixture generation.
- `docs/operations/`: preview/alpha workflow, EAS credential recovery, signing certificate verification, and release rollback.
- update `AGENTS.md` multi-surface guidance only when the Kotlin client becomes an actively maintained surface.

## Shippability gates

The PR stays draft until all apply:

- every applicable parity-tracker item is checked; every N/A item has a maintainer-approved reason;
- every screen passes the Material 3 Expressive audit; every applicable expressive component is adopted; the catalog covers the complete pinned `MaterialShapes` set and component states; and no primary flow uses a small/extra-small action;
- direct pairing and T3 Connect pass on physical Android devices;
- multi-environment, multi-device, local, relay, and tunnel cases pass;
- all provider decisions are documented and tested;
- no known credential loss, outbox loss/reordering, transcript corruption, or navigation dead end;
- JVM, contract, lint, Compose, instrumentation, screenshot, and macrobenchmark gates are green;
- signed preview APK installs and updates over the prior preview APK;
- release APK reports exactly `club.touchtech.s5code.kotlin` and the expected signing certificate;
- TalkBack, large text, dark/light/dynamic color, phone/tablet/foldable, predictive back, and hardware keyboard checks pass;
- user/internal/operations docs are complete;
- physical-device alpha smoke test passes;
- explicit maintainer approval is given to remove draft status.

## Main risks and mitigations

- **Contract drift:** generated fixtures plus path-gated CI; keep mapping at one boundary.
- **Duplicated client logic:** port behavior deliberately, but do not create a Kotlin/TypeScript transpilation system. Share wire contracts and test vectors, not runtime code.
- **T3 Connect auth gaps:** prove Clerk + DPoP in Phase 2/3 before depending on it for the whole shell.
- **Long-thread Compose regressions:** paging, stable keys, parsed-content caches, macrobenchmarks, and real-sized fixtures from the start.
- **Terminal/diff complexity:** reuse proven Android native implementations and C/JNI artifacts where licensing allows; do not rewrite terminal emulation in Compose.
- **Material Expressive API churn:** pin versions, isolate experimental APIs in `design`, maintain an API adoption ledger, and run catalog/screenshots before upgrading. Do not stay on an old API solely to avoid migrating wrappers.
- **Expression becoming visual noise or jank:** require a hero/action hierarchy per screen, use iconic shapes intentionally rather than everywhere, bound concurrent animation, honor reduced motion, and benchmark expressive components with real-sized lists.
- **EAS signing mismatch:** reuse the existing EAS project but explicitly select/configure credentials for `club.touchtech.s5code.kotlin`, verify the expected certificate fingerprint, use one controlled credential bootstrap, and never check in the keystore.
- **Preview cost/latency:** label-gated builds, concurrency cancellation, Gradle caching, and debug checks before the signed build.
- **Scope pressure:** implement vertical slices in phase order; a broad collection of static screens is not parity.
