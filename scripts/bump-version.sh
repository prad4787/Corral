#!/usr/bin/env bash
# Set the version everywhere it lives (Cargo workspace, Tauri, npm).
# Usage: scripts/bump-version.sh 0.4.0
set -euo pipefail
V="${1:?usage: bump-version.sh X.Y.Z}"
[[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a version: $V" >&2; exit 1; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
python3 -I - "$ROOT" "$V" <<'PY'
import json, re, sys
root, v = sys.argv[1], sys.argv[2]
# Cargo workspace: only the version inside [workspace.package].
p = f"{root}/Cargo.toml"
s = open(p).read()
s = re.sub(r'(\[workspace\.package\][^\[]*?\nversion = ")[^"]+(")', rf'\g<1>{v}\2', s, count=1)
open(p, "w").write(s)
# JSON files: keep formatting, replace the first top-level "version".
for p in (f"{root}/app/src-tauri/tauri.conf.json", f"{root}/app/package.json"):
    s = open(p).read()
    s = re.sub(r'("version":\s*")[^"]+(")', rf'\g<1>{v}\2', s, count=1)
    json.loads(s)  # still valid JSON
    open(p, "w").write(s)
PY
cargo update -q --workspace --manifest-path "$ROOT/Cargo.toml"
grep -m1 '^version' "$ROOT/Cargo.toml"
grep -m1 '"version"' "$ROOT/app/src-tauri/tauri.conf.json"
grep -m1 '"version"' "$ROOT/app/package.json"
