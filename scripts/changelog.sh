#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Generates the two changelogs from cliff.toml with git-cliff. This is where
# the per-file flags live, so the mise tasks, CI and the release workflow all
# go through it:
#
#   scripts/changelog.sh                    # rewrite CHANGELOG.md and sofleii/CHANGELOG.md
#   scripts/changelog.sh --check            # exit 1 if either file is stale (Unreleased aside)
#   scripts/changelog.sh --check sofleii    # just one of them (firmware | sofleii)
#   scripts/changelog.sh notes firmware     # one release's section, for gh release --notes-file
#
# The pending release: when the version a release PR bumps (firmware:
# params.firmware.version in site/hugo.toml; sofleii: version in
# sofleii/Cargo.toml) has no tag yet, the unreleased commits are rendered as
# that version (git-cliff --tag), so the release PR carries its own section
# and `notes` prints it before the tag exists. git-cliff dates a pending
# version "today", so --check ignores that one header's date.
#
# --check also ignores the Unreleased section: between releases it goes
# stale with every commit, by design. It fails when a released or pending
# version's section differs from what git-cliff generates: a release PR that
# wasn't regenerated, a hand edit, a cliff.toml change left unapplied.
#
# Needs the full history and the tags (CI: fetch-depth: 0).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# Firmware history starts at v3.0.0: v2.8.0 is only the lower bound of the
# range, so v3.0.0 is the first section and older releases are the template's
# "Earlier releases" footer. The crate's history starts with the crate.
flags_for() {
  case $1 in
    firmware)
      out=CHANGELOG.md
      range=(v2.8.0..HEAD)
      flags=(--tag-pattern '^v[0-9]+\.[0-9]+\.[0-9]+$' --exclude-path 'sofleii/**')
      tag=$(sed -n 's/^ *version = "\(v[0-9][0-9.]*\)"$/\1/p' site/hugo.toml | head -n1)
      ;;
    sofleii)
      out=sofleii/CHANGELOG.md
      range=(v3.0.0..HEAD)
      flags=(--tag-pattern '^sofleii-v[0-9]+\.[0-9]+\.[0-9]+$')
      tag=sofleii-v$(sed -n 's/^version = "\(.*\)"$/\1/p' sofleii/Cargo.toml | head -n1)
      # Only commits that touch sofleii/**, but not via --include-path:
      # git-cliff drops path-filtered commits before it assigns tags, so a
      # sofleii tag on a merge that touches nothing in sofleii/ (sofleii-v0.1.0
      # is on the v3.2.0 release merge) would vanish, and its commits would
      # land in the next version. --skip-commit keeps the tag.
      local c
      while read -r c; do
        flags+=(--skip-commit "$c")
      done < <(comm -23 <(git log --no-merges --format=%H "${range[0]}" | sort) \
        <(git log --no-merges --format=%H "${range[0]}" -- sofleii | sort))
      ;;
    *) echo "unknown changelog '$1' (firmware | sofleii)" >&2; exit 2 ;;
  esac
  [[ $tag =~ v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "no $1 version found" >&2; exit 1; }
  pending=()
  if ! git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    pending=(--tag "$tag")
  fi
}

cliff() {
  RUST_LOG=${RUST_LOG:-error} CHANGELOG_COMPONENT=$component git cliff --config cliff.toml "${flags[@]}" "$@"
}

# What --check compares: the file without its Unreleased section, and with
# the pending version's date blanked.
normalize() {
  local tag=${pending[1]:-}
  awk -v tag="$tag" '
    /^## \[Unreleased\]/ { skip = 1; next }
    /^## / { skip = 0 }
    skip { next }
    tag != "" && /^## / && index($0, tag ") - ") { sub(/ - [0-9-]+$/, " - (pending)") }
    { print }
  ' "$1"
}

mode="write"
case ${1:-} in
  --check) mode="check"; shift ;;
  notes) mode="notes"; shift ;;
esac

if [ "$mode" = notes ]; then
  component=${1:?usage: changelog.sh notes firmware|sofleii}
  flags_for "$component"
  if ((${#pending[@]})); then
    cliff --unreleased "${pending[@]}" --strip all "${range[@]}"
  else
    # The newest release's section (the tag the release workflow runs for).
    cliff --latest --strip all "${range[@]}"
  fi
  exit 0
fi

components=("$@")
((${#components[@]})) || components=(firmware sofleii)
stale=0
for component in "${components[@]}"; do
  flags_for "$component"
  note=${pending[1]:+ (pending $tag)}
  if [ "$mode" = write ]; then
    cliff "${pending[@]}" --output "$out" "${range[@]}"
    echo "$out written$note"
  else
    tmp=$(mktemp)
    cliff "${pending[@]}" --output "$tmp" "${range[@]}"
    if diff -u --label "$out (committed)" --label "$out (generated)" \
        <(normalize "$out" 2>/dev/null) <(normalize "$tmp"); then
      echo "$out is up to date$note"
    else
      echo "$out is stale: run 'mise run changelog' and commit it" >&2
      stale=1
    fi
    rm -f "$tmp"
  fi
done
exit "$stale"
