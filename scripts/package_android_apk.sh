#!/usr/bin/env bash
# Build the Android .apk with cargo-apk and stage it in dist/ with its digest.
# cargo-apk reads [package.metadata.android] from the crate manifest (label, icon
# mipmaps, permissions), so this script only checks the toolchain and collects.
#
# Inputs from the environment: TARGET (aarch64-linux-android), VERSION, PACKAGE_PREFIX.
# Toolchain: `cargo apk`, ANDROID_HOME and ANDROID_NDK_HOME; a release signature
# comes from the CARGO_APK_RELEASE_KEYSTORE_* variables when the caller sets them.
set -euo pipefail

: "${TARGET?the target triple is required}"
: "${VERSION?the version is required}"
: "${PACKAGE_PREFIX?the package prefix is required}"

if ! command -v cargo-apk >/dev/null; then
  echo "cargo-apk is missing: run cargo install cargo-apk" >&2
  exit 1
fi
: "${ANDROID_HOME?the Android SDK directory is required}"

# cargo-apk prefers ANDROID_NDK_ROOT over ANDROID_NDK_HOME and panics when the
# variable it reads points somewhere without source.properties: pin both.
android_ndk="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
if [ ! -f "${android_ndk}/source.properties" ]; then
  echo "the exported NDK path is not an NDK directory (no source.properties inside)" >&2
  echo "run scripts/build_packages.sh --platform android, or point ANDROID_NDK_HOME" >&2
  echo "at a real NDK (\$ANDROID_HOME/ndk/<version> or /opt/homebrew/share/android-ndk)" >&2
  exit 1
fi
export ANDROID_NDK_ROOT="${android_ndk}" ANDROID_NDK_HOME="${android_ndk}"

# cargo-apk never auto-generates a keystore for the release profile: without an
# explicit one, sign with the local debug key (installable, not store-ready).
if [ -z "${CARGO_APK_RELEASE_KEYSTORE:-}" ]; then
  debug_keystore="${HOME}/.android/debug.keystore"
  if [ -f "${debug_keystore}" ]; then
    echo "no CARGO_APK_RELEASE_KEYSTORE: signing with the local debug key" >&2
    export CARGO_APK_RELEASE_KEYSTORE="${debug_keystore}"
    export CARGO_APK_RELEASE_KEYSTORE_PASSWORD="android"
    export CARGO_APK_RELEASE_KEY_ALIAS="androiddebugkey"
    export CARGO_APK_RELEASE_KEY_PASSWORD="android"
  else
    echo "set CARGO_APK_RELEASE_KEYSTORE (and the *_PASSWORD/*_ALIAS variables)" >&2
    echo "or keep a debug keystore at ~/.android/debug.keystore" >&2
    exit 1
  fi
fi

# `--lib` is required, not optional: cargo-apk iterates every artifact and a
# workspace `[[bin]]` makes it panic after signing the real cdylib package.
(cd baihua-client-gui && cargo apk build --release --lib)
built="target/release/apk/baihua-client-gui.apk"
test -f "${built}"
package="${PACKAGE_PREFIX}${VERSION}-${TARGET}.apk"
mkdir -p dist
cp "${built}" "dist/${package}"
(cd dist && shasum -a 256 "${package}" > "${package}.sha256")
printf "archive=dist/%s\n" "${package}" >> "${GITHUB_ENV:-/dev/null}"
echo "built dist/${package}"
