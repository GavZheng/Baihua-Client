#!/usr/bin/env bash
# Produce the Linux .AppImage on a macOS developer machine with Docker: the
# x86_64 executable is built and the AppImage assembled inside one amd64
# container. The shared script packs the type-2 layout by hand here because
# Rosetta refuses the static-pie AppImage runtime that linuxdeploy needs.
# Inputs: VERSION, PACKAGE_PREFIX (from build_packages.sh).
set -euo pipefail

: "${VERSION?the version is required}"
: "${PACKAGE_PREFIX?the package prefix is required}"

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repository_root}"
target_triple="x86_64-unknown-linux-gnu"
mirror="${BAIHUA_APT_MIRROR:-http://mirrors.aliyun.com}"

docker run --rm \
  --platform linux/amd64 \
  --volume "${repository_root}:/io" \
  --volume "${HOME}/.cargo/registry:/usr/local/cargo/registry" \
  --workdir /io \
  rust:1-bookworm bash -euo pipefail -c "
    sed -i 's|deb.debian.org|${mirror#http://}|g' /etc/apt/sources.list.d/debian.sources
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends cmake nasm ninja-build squashfs-tools
    export CARGO_TARGET_DIR=/io/dist-target
    cargo build --release -p baihua-client-gui --target ${target_triple}
    TARGET=${target_triple} VERSION=${VERSION} PACKAGE_PREFIX=${PACKAGE_PREFIX} \
      BINARY=baihua-gui BUNDLE_NAME=Baihua CARGO_TARGET_DIR=/io/dist-target \
      bash scripts/package_linux_appimage.sh
  "
