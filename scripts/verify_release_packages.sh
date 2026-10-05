#!/usr/bin/env bash
# Local check of a freshly built macOS installer package. Run from the repository root
# after scripts/package_macos_bundle.sh: it re-opens the image, checks the digest and
# proves the packaged client runs with no repository nearby, which is the only way to
# show the shared configuration really travels inside the bundle.
#
# Inputs from the environment: TARGET, VERSION, PACKAGE_PREFIX, BUNDLE_NAME.
set -euo pipefail

: "${TARGET?the target triple is required}"
: "${VERSION?the version is required}"
: "${PACKAGE_PREFIX?the package prefix is required}"
: "${BUNDLE_NAME?the bundle name is required}"

package="${PACKAGE_PREFIX}${VERSION}-${TARGET}.dmg"
test -f "dist/${package}"
cd dist
shasum -a 256 -c "${package}.sha256"
cd ..
mount="verify-mount"
mkdir -p "${mount}"
# A crash would otherwise strand the image mounted and break the next attach.
trap 'cd /; umount -f "'"${PWD}"'/${mount}" 2>/dev/null; hdiutil detach "'"${PWD}"'/${mount}" -quiet 2>/dev/null' EXIT
hdiutil attach -nobrowse -readonly -mountpoint "${mount}" "dist/${package}" > /dev/null
app="${mount}/${BUNDLE_NAME}.app"
test -d "${app}"
test -f "${app}/Contents/Resources/config/languages/en-US.json"
test -f "${app}/Contents/Resources/config/themes/high-contrast.json"
cd /tmp
executable="${OLDPWD}/${app}/Contents/MacOS/baihua-gui"
if ! lipo -archs "${executable}" | grep -qw "$(uname -m)"; then
  echo "launch test skipped: ${executable} targets another architecture"
else
  BAIHUA_DIR="/tmp/baihua-verify" "${executable}" version
fi
cd "${OLDPWD}"
hdiutil detach "${mount}" > /dev/null
rmdir "${mount}"
echo "verified dist/${package}"
