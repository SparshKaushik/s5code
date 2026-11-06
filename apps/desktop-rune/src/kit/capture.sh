#!/usr/bin/env bash
# Run on s5w-kit; desk.mjs is the macOS Electron reference harness, installed at ~/desk.mjs.
# Output: ~/shots/kit-*.ref.{png,json}; pass the same theme to tools/snap.sh.
set -euo pipefail
for theme in dark light; do
  args=(); [[ $theme == light ]] && args+=(--light)
  node ~/desk.mjs / "kit-palette-$theme" "${args[@]}" --key Meta+K --pause 400
  node ~/desk.mjs / "kit-query-$theme" "${args[@]}" --key Meta+K --type replay --pause 400
  node ~/desk.mjs /settings/appearance "kit-settings-$theme" "${args[@]}"
  node ~/desk.mjs / "kit-context-$theme" "${args[@]}" --rclick 112,207 --pause 400
done
for theme in dark light; do
  args=(); [[ $theme == light ]] && args+=(--light)
  for scene in buttons sizes badges menu controls radio select combobox popover tooltip toggles feedback; do
    node ~/s5rune/src/kit/reference-capture.mjs / "kit-$scene-$theme" "${args[@]}" --eval "window.__kitGallery='$scene';$(cat ~/s5rune/src/kit/reference-gallery.js)" --pause 300
  done
done

# Rune counterparts. Keep each baseline PNG before rebuilding if measuring before/after.
for theme in dark light; do
  for scene in kit-buttons kit-button-sizes kit-badges kit-menu kit-context-menu kit-controls kit-radio kit-select kit-combobox kit-popover kit-tooltip kit-toggles kit-feedback kit-palette kit-palette-query; do
    ~/s5rune/tools/snap.sh "$scene" "$theme"
    suffix=""; [[ $theme == light ]] && suffix="-light"
    test -s "$HOME/shots/$scene$suffix.png"
  done
done
