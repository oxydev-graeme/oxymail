#!/usr/bin/env bash
# Bumps the version everywhere, commits, tags and pushes; GitHub Actions then builds and
# publishes the release that installed copies update from.  Usage: scripts/release.sh 0.2.1
set -euo pipefail
version="${1:?usage: scripts/release.sh <version, e.g. 0.2.1>}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
[ -z "$(git status --porcelain)" ] || { echo "Commit or stash your changes first."; exit 1; }

python3 - "$version" <<'PY'
import json, re, sys
v = sys.argv[1]
for path in ["app/src-tauri/tauri.conf.json", "app/package.json"]:
    data = json.load(open(path)); data["version"] = v
    json.dump(data, open(path, "w"), indent=2); open(path, "a").write("\n")
cargo = "app/src-tauri/Cargo.toml"
text = open(cargo).read()
open(cargo, "w").write(re.sub(r'(?m)^version = "[^"]+"', f'version = "{v}"', text, count=1))
PY
(cd app && npm install --package-lock-only --silent)
cargo update -p oxymail-app --offline >/dev/null 2>&1 || true

git commit -am "Release v$version"
git tag -a "v$version" -m "OxyMail $version"
git push && git push origin "v$version"
echo "Tagged v$version. Follow the build with: gh run watch"
