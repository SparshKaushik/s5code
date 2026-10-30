# Third-Party Notices

## Ghostty / libghostty-vt

The Kotlin Android terminal renderer vendors upstream `libghostty-vt` shared libraries and C
headers. The React Native Android and web terminal integrations use the same revision and C ABI.

- Upstream project: https://github.com/ghostty-org/ghostty
- Vendored revision: `9f62873bf195e4d8a762d768a1405a5f2f7b1697`
- License: MIT

Ghostty's MIT license applies to the vendored Android libraries. Keep this notice, the copied
per-ABI libraries, and repository-root `native/libghostty-vt` headers in sync when updating.

## KotlinTextMate and bundled TextMate grammars

Native syntax highlighting uses `io.github.ivan-magda:kotlin-textmate-core` 0.2.0, a Kotlin/JVM
port of `vscode-textmate` using Joni/Oniguruma-compatible regular expressions.

- Upstream project: https://github.com/ivan-magda/kotlin-textmate
- Version: `0.2.0`
- License: MIT
- Full license and upstream notices: `app/src/main/assets/textmate/licenses/`

The APK also bundles a selected 84-grammar dependency closure from `tm-grammars`; 60 grammars
are directly selectable through Markdown fence labels and workspace file extensions. The snapshot
is pinned to `shikijs/textmate-grammars-themes` revision
`66128354eb1cc7f944b3f05b7f4ebc5efb9fd32e`. Each grammar retains its upstream license. The
full generated attribution and license text for the selected grammar catalog—including the separately
pinned MIT Taplo TOML and Red Hat YAML grammar snapshots—is packaged at
`app/src/main/assets/textmate/licenses/TM_GRAMMARS_NOTICE.txt`; the catalog's MIT license is at
`TM_GRAMMARS_LICENSE.txt`.

## MesloLGS NF (terminal font)

- Files: `app/src/main/assets/fonts/MesloLGS-NF-{Regular,Bold}.ttf`
- Source: https://github.com/romkatv/powerlevel10k-media
- Upstream: Meslo LG by André Berg (customization of Apple's Menlo), Nerd Fonts patcher
- License: Apache License 2.0
