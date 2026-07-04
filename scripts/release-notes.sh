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

range="HEAD"
if [[ -n "$previous_tag" ]]; then
  range="${previous_tag}..HEAD"
fi

declare -A seen_prs=()
prs_file="$(mktemp)"

while IFS= read -r sha; do
  while IFS=$'\t' read -r number title user url; do
    [[ -n "${number:-}" ]] || continue
    if [[ -n "${seen_prs[$number]+x}" ]]; then
      continue
    fi
    seen_prs[$number]=1
    printf '%s\t%s\t%s\t%s\n' "$number" "$title" "$user" "$url" >> "$prs_file"
  done < <(
    gh api \
      -H "Accept: application/vnd.github+json" \
      "/repos/${repo}/commits/${sha}/pulls" \
      --jq '.[] | [.number, .title, .user.login, .html_url] | @tsv' 2>/dev/null || true
  )
done < <(git rev-list --reverse "$range")

{
  echo "## What's Changed"
  echo

  if [[ -s "$prs_file" ]]; then
    while IFS=$'\t' read -r number title user url; do
      echo "- ${title} by @${user} in ${url}"
    done < "$prs_file"
  else
    echo "No pull requests were included in this release."
  fi

  echo
  if [[ -n "$previous_tag" ]]; then
    echo "**Full Changelog**: https://github.com/${repo}/compare/${previous_tag}...${tag}"
  else
    echo "Initial release."
  fi
} > "$out"

rm -f "$prs_file"
