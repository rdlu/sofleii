#!/usr/bin/env bash
# Installs the pinned git-cliff (locally it comes from pacman, so it is not a
# mise tool) from its GitHub release's static musl tarball. The SHA-512s are
# pinned here, not fetched next to the tarball, so a replaced release asset
# fails the check. To bump: change version and both sums (each release ships
# a .tar.gz.sha512 per asset); `mise run changelog` warns when the local
# git-cliff differs from this version.
# Usage: install-git-cliff.sh DIR. In GitHub Actions, DIR is added to $GITHUB_PATH.
set -euo pipefail

version=2.14.2
declare -A sha512=(
  [x86_64]=95b0c55ab3e351ca7c0285fbbde8f1e8c42906466a173e6884f5c7ca814523bda44cbd4ab26b0aa28cace4c02c5ee6b553e9742bd9d7df47a73b9ef58ea429a9
  [aarch64]=43e8a3284e86f8a3f3931c8accf723821dcaffcad97f8ae7a90e583f5abace85614cccd077046e24466a22061bd601e6c42634e595d8e9939c6049523fb6d365
)

dir=${1:?usage: install-git-cliff.sh DIR}
arch=$(uname -m)
[ -n "${sha512[$arch]:-}" ] || { echo "unsupported architecture $arch" >&2; exit 1; }

name="git-cliff-${version}"
tarball="${name}-${arch}-unknown-linux-musl.tar.gz"
mkdir -p "$dir"
cd "$dir"
curl -fsSLO --retry 3 "https://github.com/orhun/git-cliff/releases/download/v${version}/${tarball}"
echo "${sha512[$arch]}  ${tarball}" | sha512sum -c -
tar -xzf "$tarball" --strip-components=1 "${name}/git-cliff"
rm "$tarball"
./git-cliff --version

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$PWD" >> "$GITHUB_PATH"
fi
