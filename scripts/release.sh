#!/usr/bin/env bash
set -euo pipefail

CARGO_TOML="Cargo.toml"

usage() {
  echo "Usage: $0 <patch|minor|major|VERSION>"
  echo ""
  echo "Examples:"
  echo "  $0 patch        # 0.1.0 -> 0.1.1"
  echo "  $0 minor        # 0.1.0 -> 0.2.0"
  echo "  $0 major        # 0.1.0 -> 1.0.0"
  echo "  $0 0.3.0        # set explicit version"
  exit 1
}

if [[ $# -ne 1 ]]; then
  usage
fi

if ! command -v gh &>/dev/null; then
  echo "Error: gh (GitHub CLI) is required. Install it: https://cli.github.com"
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "Error: working directory is not clean. Commit or stash changes first."
  exit 1
fi

current_version=$(grep '^version' "$CARGO_TOML" | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "Current version: $current_version"

IFS='.' read -r major minor patch <<< "$current_version"

case "$1" in
  patch) new_version="$major.$minor.$((patch + 1))" ;;
  minor) new_version="$major.$((minor + 1)).0" ;;
  major) new_version="$((major + 1)).0.0" ;;
  *)
    if [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
      new_version="$1"
    else
      echo "Error: invalid version '$1'"
      usage
    fi
    ;;
esac

echo "New version:     $new_version"
echo ""
read -rp "Proceed? [y/N] " confirm
if [[ "$confirm" != [yY] ]]; then
  echo "Aborted."
  exit 0
fi

sed -i '' "s/^version = \"$current_version\"/version = \"$new_version\"/" "$CARGO_TOML"

cargo check --quiet 2>/dev/null || true

git add "$CARGO_TOML"
# Also stage Cargo.lock if it exists and changed
if [[ -f Cargo.lock ]] && ! git diff --quiet Cargo.lock 2>/dev/null; then
  git add Cargo.lock
fi

git commit -m "chore: bump version to $new_version"
git tag "v$new_version"

echo ""
echo "Pushing to remote..."
git push
git push origin "v$new_version"

echo ""
echo "Creating GitHub release..."
gh release create "v$new_version" --generate-notes --title "v$new_version"

echo ""
echo "Done! Release v$new_version created."
echo "CI will build and attach binaries automatically."
