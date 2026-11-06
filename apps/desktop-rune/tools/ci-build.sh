#!/bin/bash
# Build this checkout's HEAD on GitHub Actions (rune-desktop-linux.yml) and download the bundle
# that tools/modal_render.py renders. Commit first: CI builds what is pushed, not the worktree.
#   tools/ci-build.sh [OUT_DIR]   -> prints OUT_DIR/bundle.tgz (exit 1 with the failing log tail)
set -euo pipefail
out=${1:-$(mktemp -d)}
sha=$(git rev-parse HEAD)
branch=$(git rev-parse --abbrev-ref HEAD)
case "$branch" in feat/rune-*) ref=$branch ;; *) ref=feat/rune-ci/$branch ;; esac
git push -q --force origin "HEAD:refs/heads/$ref" >&2
wf=rune-desktop-linux.yml
run=""
for _ in $(seq 1 12); do
  run=$(gh run list --workflow "$wf" --commit "$sha" --limit 1 --json databaseId -q '.[0].databaseId // empty')
  [ -n "$run" ] && break
  sleep 5
done
if [ -z "$run" ]; then
  gh workflow run "$wf" --ref "$ref" >&2
  for _ in $(seq 1 24); do
    sleep 5
    run=$(gh run list --workflow "$wf" --commit "$sha" --limit 1 --json databaseId -q '.[0].databaseId // empty')
    [ -n "$run" ] && break
  done
fi
[ -n "$run" ] || { echo "no CI run for $sha on $ref" >&2; exit 1; }
echo "CI run $run ($ref @ ${sha:0:10})" >&2
if ! gh run watch "$run" --exit-status --interval 20 >/dev/null 2>&1; then
  gh run view "$run" --log-failed 2>/dev/null | grep -E "error(\[|:)|-->|panicked|FAILED|^ *= " | head -80 >&2
  exit 1
fi
mkdir -p "$out"
rm -f "$out/bundle.tgz"
gh run download "$run" -n "s5code-rune-linux-$sha" -D "$out" >&2
echo "$out/bundle.tgz"
