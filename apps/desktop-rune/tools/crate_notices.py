#!/usr/bin/env python3
"""The native app's Rust crate notices, in the web's third-party-licenses.json format.

Run from apps/desktop-rune after `cargo fetch --locked`; writes the manifest to the path given.
Every crate the app links (normal dependencies, for the build host's platform, workspace members
excluded) becomes one `crate` entry in the `native` bundle. Notice files follow the web
generator's rules (scripts/lib/third-party-licenses.ts): LICENSE/LICENCE/COPYING/NOTICE files in
the package root and up to two folders down; a crate that ships none borrows them from its
repository's other crates, then from its git checkout's root.
"""

import json
import os
import re
import subprocess
import sys

NOTICE = re.compile(r"^(?:licen[cs]e|copying|notice)(?:[._-].*)?$", re.I)
EXTENSIONS = {"", ".0bsd", ".agpl", ".apache2", ".bsd", ".gpl", ".isc", ".lgpl", ".markdown", ".md",
              ".mit", ".mpl", ".mpl2", ".rst", ".txt", ".unlicense"}
SKIP_DIRS = {".git", "target", "node_modules"}


def is_notice(name):
    return bool(NOTICE.match(name)) and os.path.splitext(name)[1].lower() in EXTENSIONS


def notice_files(root, depth=2):
    found = []

    def walk(rel, level):
        try:
            entries = sorted(os.scandir(os.path.join(root, rel)), key=lambda e: e.name)
        except OSError:
            return
        for entry in entries:
            path = os.path.join(rel, entry.name) if rel else entry.name
            if entry.is_file() and is_notice(entry.name):
                found.append(path)
            elif entry.is_dir() and entry.name not in SKIP_DIRS and level > 0:
                walk(path, level - 1)

    walk("", depth + 1)
    return sorted(found)


def notice_text(root, depth=2):
    files = notice_files(root, depth)
    sections = []
    for name in files:
        try:
            with open(os.path.join(root, name), encoding="utf-8", errors="replace") as f:
                contents = f.read().strip()
        except OSError:
            continue
        if contents:
            sections.append(contents if len(files) == 1 else f"{name}\n\n{contents}")
    return "\n\n---\n\n".join(sections) or None


def http_url(value):
    return value if isinstance(value, str) and re.match(r"^https?://", value) else None


def checkout_root(manifest_dir):
    """The git checkout a git dependency lives in (~/.cargo/git/checkouts/<repo>/<rev>)."""
    parts = os.path.normpath(manifest_dir).split(os.sep)
    if "checkouts" in parts:
        i = parts.index("checkouts")
        if len(parts) > i + 2:
            return os.sep.join(parts[: i + 3])
    return None


def host_triple():
    out = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    return next(line.split(": ", 1)[1] for line in out.splitlines() if line.startswith("host: "))


def main(out_path):
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--offline",
         "--filter-platform", host_triple()],
        check=True, capture_output=True, text=True).stdout)
    packages = {p["id"]: p for p in metadata["packages"]}
    members = set(metadata["workspace_members"])
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}

    # Only what links into the app: normal dependencies, followed transitively.
    linked, stack = set(), [metadata["resolve"]["root"]]
    while stack:
        node = nodes[stack.pop()]
        for dep in node["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]) and dep["pkg"] not in linked:
                linked.add(dep["pkg"])
                stack.append(dep["pkg"])
    crates = sorted((packages[i] for i in linked - members), key=lambda p: (p["name"].lower(), p["version"]))

    own = {p["id"]: notice_text(os.path.dirname(p["manifest_path"])) for p in crates}
    by_repo = {}
    for p in crates:
        repo = (p.get("repository") or "").lower().rstrip("/")
        if repo and own[p["id"]] and repo not in by_repo:
            by_repo[repo] = own[p["id"]]

    entries = []
    for p in crates:
        license_name = p.get("license") or ("See notice" if p.get("license_file") else "Unknown")
        text = own[p["id"]]
        if not text:
            text = by_repo.get((p.get("repository") or "").lower().rstrip("/"))
        if not text:
            root = checkout_root(os.path.dirname(p["manifest_path"]))
            text = root and notice_text(root, depth=0)
        if not text:
            text = f"{p['name']} {p['version']} is licensed under {license_name}. Its package includes no notice file."
        entries.append({
            "bundles": ["native"],
            "kind": "crate",
            "license": license_name,
            "name": p["name"],
            "noticeText": text,
            "sourceUrl": http_url(p.get("repository")) or http_url(p.get("homepage")),
            "version": p["version"],
        })
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump({"schemaVersion": 1, "entries": entries}, f, ensure_ascii=False, separators=(",", ":"))
    missing = sum(1 for e in entries if e["noticeText"].endswith("includes no notice file."))
    print(f"crate notices: {len(entries)} crates, {missing} without a notice file -> {out_path}")


if __name__ == "__main__":
    main(sys.argv[1])
