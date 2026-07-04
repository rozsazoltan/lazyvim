#!/usr/bin/env bash
set -euo pipefail

version="${1:?version is required}"
tag="v${version}"
out="${2:-dist/release-notes.md}"
repo="${GITHUB_REPOSITORY:-rozsazoltan/lazyvim}"

mkdir -p "$(dirname "$out")"
previous_tag=""
if previous_tag="$(git describe --tags --abbrev=0 --match 'v*' 2>/dev/null)"; then
  :
else
  previous_tag=""
fi

{
  echo "## What's Changed"
  echo

  if [[ -n "$previous_tag" ]]; then
    git log "${previous_tag}..HEAD" --pretty=format:'- %s (%h)' --no-merges || true
  else
    git log --pretty=format:'- %s (%h)' --no-merges || true
  fi

  echo
  echo
  if [[ -n "$previous_tag" ]]; then
    echo "**Full Changelog**: https://github.com/${repo}/compare/${previous_tag}...${tag}"
  else
    echo "Initial release."
  fi
} > "$out"
