#!/usr/bin/env bash
# Assemble an AppDir and produce one .AppImage file. Prefers linuxdeploy when
# it is runnable (the CI case); otherwise packs the type-2 layout by hand —
# runtime blob, zero padding to 4096, squashfs — because under CPU emulation
# neither linuxdeploy nor the AppImage runtime can be executed.
# Inputs from the environment: TARGET, VERSION, PACKAGE_PREFIX, BINARY, BUNDLE_NAME,
# optional LINUXDEPLOY (path to the linuxdeploy executable).
set -euo pipefail

: "${TARGET?the target triple is required}"
: "${VERSION?the version is required}"
: "${PACKAGE_PREFIX?the package prefix is required}"
: "${BINARY?the release binary name is required}"
: "${BUNDLE_NAME?the bundle name is required}"
LINUXDEPLOY="${LINUXDEPLOY:-}"
release_directory="${CARGO_TARGET_DIR:-target}/${TARGET}/release"
appdir="AppDir-${TARGET}"
image="${PACKAGE_PREFIX}${VERSION}-${TARGET}.AppImage"
rm -rf "${appdir}" "${image}" "${image}.sha256"
bindir="${appdir}/usr/bin"
sharedir="${appdir}/usr/share/Baihua"
desktopdir="${appdir}/usr/share/applications"
icondir="${appdir}/usr/share/icons/hicolor/256x256/apps"
metainfodir="${appdir}/usr/share/metainfo"
mkdir -p "${bindir}" "${sharedir}/config" "${desktopdir}" "${icondir}" "${metainfodir}"
cp "${release_directory}/${BINARY}" "${bindir}/${BINARY}"
chmod 755 "${bindir}/${BINARY}"
cp -R config "${sharedir}/config"
cp baihua-client-gui/assets/images/icon.png "${appdir}/Baihua.png"
cp baihua-client-gui/assets/images/icon.png "${icondir}/Baihua.png"
ln -sf Baihua.png "${appdir}/.DirIcon"
desktop="${appdir}/Baihua.desktop"
printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Baihua' 'GenericName=Instant messenger' "Exec=${BINARY} %U" 'Terminal=false' 'Icon=Baihua' 'Categories=Network;InstantMessaging;' 'StartupWMClass=baihua-gui' > "${desktop}"
cp "${desktop}" "${desktopdir}/Baihua.desktop"
cp scripts/linux_apprun.sh "${appdir}/AppRun"
chmod +x "${appdir}/AppRun"

pack_type_two_by_hand() {
  runtime="appimage-runtime-x86_64"
  if [ ! -f "${runtime}" ]; then
    curl --http1.1 -fL --retry 6 --retry-all-errors --retry-delay 3 -o "${runtime}" \
      "${APPIMAGE_RUNTIME_URL:-https://github.com/AppImage/type2-runtime/releases/latest/download/runtime-x86_64}"
  fi
  size="$(stat -c '%s' "${runtime}")"
  pad=$(( (4096 - size % 4096) % 4096 ))
  squashfs="${appdir}.squashfs"
  mksquashfs "${appdir}" "${squashfs}" -noappend -all-root -comp xz > /dev/null
  cp "${runtime}" "${image}"
  dd if=/dev/zero bs=1 count="${pad}" >> "${image}" 2>/dev/null
  cat "${squashfs}" >> "${image}"
  chmod +x "${image}"
  rm -f "${squashfs}"
}

if [ -x "${LINUXDEPLOY}" ] && "${LINUXDEPLOY}" --appdir "${appdir}" --output appimage; then
  # linuxdeploy names the output from the desktop entry plus the machine
  # architecture, so adopt any produced image instead of guessing the name.
  for candidate in ./*.AppImage; do
    if [ -f "${candidate}" ]; then mv "${candidate}" "${image}"; fi
  done
fi
# No image exists when linuxdeploy was unusable or produced nothing to adopt.
[ -f "${image}" ] || pack_type_two_by_hand

if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "${image}" > "${image}.sha256"
else
  sha256sum "${image}" > "${image}.sha256"
fi
mkdir -p dist
mv "${image}" "${image}.sha256" dist/
printf "archive=dist/%s\n" "${image}" >> "${GITHUB_ENV:-/dev/null}"
echo "built dist/${image}"
