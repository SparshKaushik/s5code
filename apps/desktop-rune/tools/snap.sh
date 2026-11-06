#!/bin/bash
# On a build VM: render a scene headless at the reference's size.
# usage: tools/snap.sh SCENE [dark|light] [extra rune args...]  -> ~/shots/SCENE.png (+ .tree.json)
scene=$1; theme=${2:-dark}; shift 2 2>/dev/null
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
mkdir -p ~/shots
out=~/shots/$scene$([ "$theme" = light ] && echo -light)
"$(dirname "$0")/../target/release/s5code-rune" --snap "$out.png" --tree "$out.tree.json" --scene "$scene" --size 1440x900 --scale 1 --theme "$theme" "$@" 2>&1 | grep -v "^rune: " || true
echo "$out.png"
