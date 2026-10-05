#!/usr/bin/env bash
# Produce the Windows package on a macOS/Linux developer machine with Docker:
# stage one cross-compiles baihua-gui.exe with MinGW inside a Rust container,
# stage two renders the .msi with the wixl engine inside an Ubuntu container.
# The scripts it runs are the same ones CI uses, so local output and CI output
# stay one implementation. Inputs: VERSION, PACKAGE_PREFIX (from build_packages.sh).
set -euo pipefail

: "${VERSION?the version is required}"
: "${PACKAGE_PREFIX?the package prefix is required}"

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repository_root}"
target_triple="x86_64-pc-windows-gnu"
mirror="${BAIHUA_APT_MIRROR:-http://mirrors.aliyun.com}"

docker run --rm \
  --volume "${repository_root}:/io" \
  --volume "${HOME}/.cargo/registry:/usr/local/cargo/registry" \
  --workdir /io \
  rust:1-bookworm bash -euo pipefail -c "
    sed -i 's|deb.debian.org|${mirror#http://}|g' /etc/apt/sources.list.d/debian.sources
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 nasm cmake ninja-build
    rustup target add ${target_triple}
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
    export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
    export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++
    export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
    cargo build --release -p baihua-client-gui --target ${target_triple}
  "

docker run --rm \
  --volume "${repository_root}:/io" \
  --workdir /io \
  ubuntu:24.04 bash -euo pipefail -c "
    sed -i 's|http://archive.ubuntu.com/ubuntu/|${mirror}/ubuntu/|g;s|http://ports.ubuntu.com/ubuntu-ports/|${mirror}/ubuntu-ports/|g' /etc/apt/sources.list.d/ubuntu.sources
    apt-get update -qq
    apt-get install -y -qq python3 wixl msitools
    TARGET=${target_triple} VERSION=${VERSION} PACKAGE_PREFIX=${PACKAGE_PREFIX} \
      BINARY=baihua-gui.exe BUNDLE_NAME=Baihua bash scripts/package_windows_installer.sh
  "
