#!/bin/bash
# Build this checkout's HEAD on GitHub Actions (rune-desktop-linux.yml) and download the bundle
# that tools/modal_render.py renders. Commit first: CI builds what is pushed, not the worktree.
#   tools/ci-build.sh [OUT_DIR]   -> prints OUT_DIR/bundle.tgz (exit 1 with the failing log tail);
#   OUT_DIR/s5code-server is the Linux server binary that tools/modal_live.py runs.
set -euo pipefail
out=${1:-$(mktemp -d)}
sha=$(git rev-parse HEAD)
branch=$(git rev-parse --abbrev-ref HEAD)
case "$branch" in feat/rune-*) ref=$branch ;; *) ref=feat/rune-ci/$branch ;; esac
git push -q --force origin "HEAD:refs/heads/$ref" >&2
# Push-triggered only (workflow_dispatch needs the file on the default branch), so HEAD must
# touch apps/desktop-rune; look the run up by commit and workflow name.
name="Rune desktop (Linux build for headless scenes)"
run=""
for _ in $(seq 1 24); do
  run=$(gh run list --commit "$sha" --limit 20 --json databaseId,workflowName -q "[.[] | select(.workflowName == \"$name\")][0].databaseId // empty")
  [ -n "$run" ] && break
  sleep 5
done
[ -n "$run" ] || { echo "no CI run for $sha on $ref" >&2; exit 1; }
echo "CI run $run ($ref @ ${sha:0:10})" >&2
if ! gh run watch "$run" --exit-status --interval 20 >/dev/null 2>&1; then
  gh run view "$run" --log-failed 2>/dev/null | grep -E "error(\[|:)|-->|panicked|FAILED|^ *= " | head -80 >&2
  exit 1
fi
mkdir -p "$out"
rm -f "$out/bundle.tgz"
rm -f "$out/s5code-server"
gh run download "$run" -n "s5code-rune-linux-$sha" -D "$out" >&2
gh run download "$run" -n "s5code-server-linux-$sha" -D "$out" >&2
echo "$out/bundle.tgz"
