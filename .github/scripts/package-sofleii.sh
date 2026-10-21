#!/usr/bin/env bash
# Packages a sofleii release binary as
#   <out>/sofleii-<version>-<target>.tar.gz        (one top-level directory:
#                                                    sofleii, LICENSE, README.md)
#   <out>/sofleii-<version>-<target>.tar.gz.sha256 (`sha256sum -c` format)
# The name is what mise's github: backend matches on (OS + arch in the name).
# Run from the repo root; used by .github/workflows/sofleii-release.yml.
#
# usage: package-sofleii.sh <version> <target> <binary> <out-dir>
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <target> <binary> <out-dir>" >&2
  exit 2
fi
version=$1 target=$2 bin=$3 out=$4
name="sofleii-${version}-${target}"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name"
install -m 0755 "$bin" "$stage/$name/sofleii"
install -m 0644 LICENSE "$stage/$name/LICENSE"
cat > "$stage/$name/README.md" <<EOF
# sofleii ${version} (${target})

Sofle Interactive Interface: a CLI and TUI for the Eyelash Sofle keyboard
over ZMK Studio RPC (USB serial or BLE through BlueZ). Linux only.

- Run \`sofleii\` in a terminal for the dashboard, or \`sofleii --help\`.
- \`sofleii --version\` names the firmware line this build speaks to.
- \`sofleii update check\` compares the firmware and sofleii itself with
  the latest releases.

Docs: https://sofleii.dlu.one/docs/sofleii/
Source and releases: https://github.com/rdlu/sofleii
License: MIT (see LICENSE).
EOF

mkdir -p "$out"
# Reproducible archive: fixed owner, order and mtime, no gzip timestamp.
tar -C "$stage" --owner=0 --group=0 --numeric-owner --sort=name \
  --mtime="@${SOURCE_DATE_EPOCH:-0}" -cf - "$name" | gzip -9n > "$out/$name.tar.gz"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "$out/$name.tar.gz"
