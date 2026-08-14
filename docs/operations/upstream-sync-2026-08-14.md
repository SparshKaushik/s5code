# Changelog: upstream sync 2026-08-14

Upstream `og/main` `7e01d33f0` (66 commits since the fork's merge-base
`f5fce741`, #6061) merged into the fork on `main` as `65d787b2a`.

## Pull requests (the big feature)

- `b28f9bf0a` feat(web): pull request surfaces — filters & qualifiers,
  all-server listing, update branch, reactions, in-place editing, smarter diffs (#6039)
- `db1507e98` feat: allow disabling auto-settle on merge (#5880)
- `d0b8d6306` feat(connect): deregister account environments from any client (#4844)
- `2eb099fdc` fix(web): cmd+click sidebar PR numbers open in the browser (#6378)
- `8d24b5131` fix(web): open modified PR clicks in browser (#6278)
- `92d4a2e99` fix(web): scope pull request errors to their environment (#6490)
- `97db94c9b` fix(web): keep pull request panel within viewport (#6451)
- `df19f6cfe` fix(server): align Codex collaboration prompts (#6432)

## Usage page

- `c842c6f5b` feat(web): hourly past-24-hour usage view (#6170)
- `6c6d2a5f` (usage hourly window + `UsageResolution` in contracts/shared)

## Theme & appearance

- `083fa4ab2` feat(web): use OKLCH for theme palettes (#6036)
- `b30a9bc41` feat(web): make environment artwork theme aware (#6183)
- `9666b8751` fix(web): preserve appearance mode when changing themes (#6343)
- `f131228a5` fix(web): theme Clerk surfaces (#6300)
- `23d45d914` fix(web): restore default stage artwork colors (#6535)
- `ac4780f45` fix(web): restore typography font sizes to defaults (#6172)

## Sidebar & navigation

- `f0b57ca23` feat(web): add Open VSX theme search (#5654)
- `44621c345` feat(web): back buttons for pull requests and usage pages in the sidebar footer (#6031)
- `52e5a75a8` feat(web): compact sidebar footer actions (#6210)
- `b73232bdd` feat(web): reset sidebar width on double click (#6320)
- `560d4a456` fix(web): keep sidebar wordmark visible at minimum width (#6246)
- `5ff3a03ad` fix(web): align sidebar wordmark label (#6086)
- `57b105267` fix(web): thread error banner dismiss survives reconnect and rerenders (#6123)

## Desktop

- `710fd0eeb` feat(desktop): add favicons to the Browser panel (#5644)
- `7e01d33f0` perf(build): stop unpacking node_modules wholesale from the Windows asar (#5877)
- `e15f655ba` fix(web): show background policy tooltips sooner (#6506)
- `59be6f784` fix(web): simplify the desktop-managed server update banner copy (#6549)

## Mobile

- `85389b988` Nest mobile task settings in bottom sheets (#6224)
- `d37a9b09b` feat(mobile): add thread title regeneration (#6253)
- `bad1143b0` fix(mobile): show a real settings cog in the Android sidebar header (#6520)
- `83ad26c3a` fix(mobile): prevent invalid HTML entities from crashing markdown (#6495)
- `fd51561b4` fix(mobile): extend blockquotes across wrapped lines (#6482)
- `6676f9c83` fix(mobile): stabilize thread composer and interactions (#5986)
- `e1378a1f4` fix(mobile): keep ordered lists inside user bubbles (#6154)
- `3da7f9c5c` fix(mobile): guard App Store release versions (#6177)
- `5304f3e9d` chore(mobile): bump app version to 1.0.4

## Web fixes

- `9fd788b5a` fix(preview): only show browser-ready local servers (#6021)
- `65b005f1e` feat(web): Copy Thread ID in thread context menu (#5574)
- `752acbf65` feat(web): create a new thread with shift+click + shortcut tooltip (#5994)
- `ac1264e2c` feat(web): project favicon and workspace icons in command subtitles (#6330)
- `b54bfc931` feat(web): a better right panel empty state (#6258)
- `5015d7cf9` fix(web): keep turn minimap stable as composer grows (#6414)
- `6bc6cb6be` fix(web): keep diff file lists scrollable past expanded files (#6423)
- `1e59b4c40` fix(web): keep the typed prompt when a draft changes repo (#6393)
- `33f970592` fix(web): make reset zoom hover visible (#6385)
- `da6253b3d` fix(web): source control scan on relay environments (#6230)
- `770946d02` fix(web): render tooltips above dropdowns (#6241)
- `860179723` fix(web): align update toast release notes link (#6322)
- `e321667b1` fix(web): prevent changed files header overlap (#6314)
- `5a8461480` fix(web): align the composer model picker (#6252)
- `c196f422e` fix(web): clean up composer resize animation (#6209)
- `2db08457f` fix(web): use upload icon for disabled push action (#6207)
- `35172010b` fix(web): clearer pull action icon (#6194)
- `1e355a2a3` fix(web): render dropdowns above toasts (#6165)
- `96bfa67b3` fix(web): align the snoozed thread wake icon (#6215)
- `2fab18e28` fix(web): show unlinked icon when viewport aspect ratio is unlocked (#6509)
- `1b16ed663` fix(web): avoid Clerk close button overlap (#6442)

## Shared / contracts

- `220e573b1` fix(shared): detect Azure DevOps SSH remotes (ssh.dev.azure.com) (#6187)
- `6befe42eb` fix(shared): normalize a bare Windows drive root (#6189)
- `849bac894` fix(connect): preserve CLI OAuth parameters through browser sign-in (#6285)

## Process / release

- `9e201941a` Remove rebase requirement before opening PR (#6479)
- `2ab188f1c` fix: ignore pull request actions in latency tracker (#6476)
- `9513e62e2` Add bil0000 to VOUCHED contributors (#6462)
- `63e6faef6` chore: add dara to vouched (#6259)
