#!/usr/bin/env bash
# Prepare and publish a semantic version release from main.

set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

require_command cargo
require_command git
require_command make
require_command python3

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: not inside a git repository" >&2
  exit 1
fi

cd "$(git rev-parse --show-toplevel)"

if [[ -n "$(git status --porcelain=v1 --untracked-files=all)" ]]; then
  echo "error: working tree is not clean; commit or stash changes first" >&2
  git status --short
  exit 1
fi

branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [[ "$branch" != "main" ]]; then
  echo "error: releases must be created from main (current branch: '${branch:-detached HEAD}')" >&2
  exit 1
fi

if ! git remote get-url origin >/dev/null 2>&1; then
  echo "error: git remote 'origin' is not configured" >&2
  exit 1
fi

upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
if [[ "$upstream" != "origin/main" ]]; then
  echo "error: main must track origin/main" >&2
  exit 1
fi

if ! remote_refs="$(git ls-remote --heads --tags --refs origin)"; then
  echo "error: could not read refs from origin" >&2
  exit 1
fi

remote_commit="$(printf '%s\n' "$remote_refs" | awk '$2 == "refs/heads/main" { print $1; exit }')"
if [[ -z "$remote_commit" ]]; then
  echo "error: origin/main was not found" >&2
  exit 1
fi

local_commit="$(git rev-parse HEAD)"
if [[ "$local_commit" != "$remote_commit" ]]; then
  echo "error: local main must match origin/main before releasing" >&2
  echo "  local:  $local_commit" >&2
  echo "  remote: $remote_commit" >&2
  exit 1
fi

manifest_version="$(cargo metadata --locked --no-deps --format-version 1 \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
if [[ ! "$manifest_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "error: Cargo.toml version must use MAJOR.MINOR.PATCH (got '$manifest_version')" >&2
  exit 1
fi

latest="$(
  {
    git tag --list 'v*'
    printf '%s\n' "$remote_refs" \
      | awk '$2 ~ /^refs\/tags\/v/ { sub("refs/tags/", "", $2); print $2 }'
  } | python3 -c '
import re
import sys

pattern = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
versions = []
for value in sys.stdin:
    tag = value.strip()
    match = pattern.fullmatch(tag)
    if match:
        versions.append((tuple(map(int, match.groups())), tag))
print(max(versions)[1] if versions else "")
'
)"

if [[ -z "$latest" ]]; then
  proposed="v${manifest_version}"
  echo "No existing semantic version tags found."
else
  proposed="$(python3 - "$latest" <<'PY'
import sys

major, minor, patch = map(int, sys.argv[1][1:].split("."))
print(f"v{major}.{minor}.{patch + 1}")
PY
)"
  echo "Latest semantic version tag: $latest"
fi

echo "Cargo.toml version: $manifest_version"
echo "Proposed release: $proposed"
echo
printf "Version to release [%s]: " "$proposed"
input=""
read -r input || true

version="${input:-$proposed}"
if [[ "$version" != v* ]]; then
  version="v${version}"
fi

if [[ ! "$version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "error: version must use vMAJOR.MINOR.PATCH (got '$version')" >&2
  exit 1
fi

if [[ -n "$latest" ]] && ! python3 - "$version" "$latest" <<'PY'
import sys

target = tuple(map(int, sys.argv[1][1:].split(".")))
latest = tuple(map(int, sys.argv[2][1:].split(".")))
raise SystemExit(0 if target > latest else 1)
PY
then
  echo "error: version '$version' must be newer than '$latest'" >&2
  exit 1
fi

if git show-ref --verify --quiet "refs/tags/${version}"; then
  echo "error: tag '$version' already exists locally" >&2
  exit 1
fi

if printf '%s\n' "$remote_refs" \
  | awk -v ref="refs/tags/${version}" '$2 == ref { found = 1 } END { exit !found }'; then
  echo "error: tag '$version' already exists on origin" >&2
  exit 1
fi

release_version="${version#v}"
short_commit="$(git rev-parse --short HEAD)"
if [[ "$release_version" == "$manifest_version" ]]; then
  commit_plan="none, Cargo.toml already has the selected version"
else
  commit_plan="Release $version"
fi

echo
echo "Release plan:"
echo "  project:          ReelMux"
echo "  manifest version: $manifest_version -> $release_version"
echo "  tag:              $version"
echo "  branch:           main"
echo "  upstream:         origin/main"
echo "  current commit:   $short_commit"
echo "  checks:           make check"
echo "  commit:           $commit_plan"
echo "  push:             main and tag to origin, atomically"
echo "  GitHub Release:   created by GitHub Actions"
echo
printf "Proceed? [y/N] "
confirm=""
read -r confirm || true
case "$confirm" in
  y|Y|yes|YES) ;;
  *)
    echo "Aborted."
    exit 1
    ;;
esac

manifest_changed=0
if [[ "$release_version" != "$manifest_version" ]]; then
  python3 - "$manifest_version" "$release_version" <<'PY'
from pathlib import Path
import re
import sys

expected, selected = sys.argv[1:]
path = Path("Cargo.toml")
text = path.read_text(encoding="utf-8")
pattern = re.compile(r'(?m)^version = "([^"]+)"$')
match = pattern.search(text)
if match is None or match.group(1) != expected:
    raise SystemExit("Cargo.toml version changed while preparing the release")
path.write_text(pattern.sub(f'version = "{selected}"', text, count=1), encoding="utf-8")
PY
  cargo update --workspace --offline --quiet
  manifest_changed=1
fi

make check

unexpected="$(git status --porcelain=v1 --untracked-files=all \
  | awk 'substr($0, 4) != "Cargo.toml" && substr($0, 4) != "Cargo.lock"')"
if [[ -n "$unexpected" ]]; then
  echo "error: release preparation produced unexpected working tree changes" >&2
  git status --short
  exit 1
fi

if (( manifest_changed )); then
  git add Cargo.toml Cargo.lock
  git commit -m "Release $version"
else
  if [[ -n "$(git status --porcelain=v1 --untracked-files=all)" ]]; then
    echo "error: checks changed the working tree" >&2
    git status --short
    exit 1
  fi
  echo "Cargo.toml already contains version $release_version; no commit is needed."
fi

git tag -a "$version" -m "Release $version"

if ! git push --atomic origin \
  "HEAD:refs/heads/main" \
  "refs/tags/${version}:refs/tags/${version}"; then
  echo "error: atomic push failed; the local commit and tag were kept" >&2
  exit 1
fi

echo
echo "Release tag $version was published with main."
echo "GitHub Actions will validate, build and publish the release."
