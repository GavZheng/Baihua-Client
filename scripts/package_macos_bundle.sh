#!/usr/bin/env bash
# Assemble Baihua.app from a compiled release binary and wrap it into a .dmg.
# The release workflow and scripts/verify_release_packages.sh both call this, so
# the packaged layout comes from exactly one implementation.
#
# Inputs from the environment: TARGET, VERSION, PACKAGE_PREFIX, BINARY, BUNDLE_NAME.
set -euo pipefail
: "${TARGET?the target triple is required}"
: "${VERSION?the version is required}"
: "${PACKAGE_PREFIX?the package prefix is required}"
: "${BINARY?the release binary name is required}"
: "${BUNDLE_NAME?the bundle name is required}"
release_directory="target/${TARGET}/release"
app="${BUNDLE_NAME}.app"
contents="${app}/Contents"
program_directory="${contents}/MacOS"
resources_directory="${contents}/Resources"
image="${PACKAGE_PREFIX}${VERSION}-${TARGET}.dmg"

# Start from an empty bundle: a leftover directory from a previous run would
# quietly ship old files inside the new version.
rm -rf "${app}" "${image}"
mkdir -p "${program_directory}" "${resources_directory}"
cp "${release_directory}/${BINARY}" "${program_directory}/${BINARY}"
chmod 755 "${program_directory}/${BINARY}"
cp "baihua-client-gui/assets/baihua.icns" "${resources_directory}/baihua.icns"
# The shared configuration must ride inside the bundle: with no repository
# nearby, Contents/Resources/config is the only place the app can read it.
cp -R config "${resources_directory}/config"

# The bundle description mirrors [package.metadata.bundle] in the graphical Cargo.toml.
cat > "${contents}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>${BUNDLE_NAME}</string>
  <key>CFBundleDisplayName</key><string>${BUNDLE_NAME}</string>
  <key>CFBundleExecutable</key><string>${BINARY}</string>
  <key>CFBundleIdentifier</key><string>com.baihua.client.gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleIconFile</key><string>baihua</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

# A signing identity is optional: without one the image still builds, and the
# release notes tell people how to open an unsigned app on their first launch.
if [ -n "${CODESIGN_IDENTITY-}" ]; then
  codesign --force --deep --options runtime --sign "${CODESIGN_IDENTITY}" "${app}"
else
  echo "no CODESIGN_IDENTITY: the bundle is left unsigned"
fi

# Drag-to-Applications layout over the shipped background picture, written by
# dmgbuild (no Finder automation); plain hdiutil is the fallback without it.
background_image="${PWD}/baihua-client-gui/assets/images/dmg.png"
if [ ! -f "${background_image}" ]; then
  echo "${background_image} is missing; the image cannot carry its background" >&2
  exit 1
fi
builder_python=""
for interpreter in "${DMGBUILD_PYTHON:-}" python3; do
  [ -n "${interpreter}" ] || continue
  command -v "${interpreter}" >/dev/null 2>&1 || continue
  if "${interpreter}" -c 'import dmgbuild' >/dev/null 2>&1; then
    builder_python="${interpreter}"
    break
  fi
done

if [ -n "${builder_python}" ]; then
  # Finder draws a dmg background unscaled, so the oversized artwork is resized here
  # to exact 1x and 2x copies; dmgbuild merges the @2x sibling into a HiDPI image.
  background_directory="dmg-background-${TARGET}"
  rm -rf "${background_directory}"
  mkdir -p "${background_directory}"
  sips -z 640 640 "${background_image}" \
    --out "${background_directory}/background.png" >/dev/null
  sips -z 1280 1280 "${background_image}" \
    --out "${background_directory}/background@2x.png" >/dev/null
  settings="dmg-settings-${TARGET}.py"
  cat > "${settings}" <<SETTINGS
format = "UDZO"
files = ["${PWD}/${app}"]
symlinks = {"Applications": "/Applications"}
icon_locations = {"${app}": (170, 330), "Applications": (470, 330)}
window_rect = ((100, 100), (640, 640))
icon_size = 96
text_size = 14
background = "${PWD}/${background_directory}/background.png"
SETTINGS
  rm -f "${image}"
  "${builder_python}" -m dmgbuild -s "${settings}" "${BUNDLE_NAME}" "${image}"
  rm -f "${settings}"
  rm -rf "${background_directory}"
else
  echo "no python3 with dmgbuild (pip install dmgbuild or set DMGBUILD_PYTHON):" >&2
  echo "the image is built without the background layout" >&2
  staging="image-${TARGET}"
  rm -rf "${staging}"
  mkdir -p "${staging}"
  cp -R "${app}" "${staging}/"
  ln -s /Applications "${staging}/Applications"
  hdiutil create -volname "${BUNDLE_NAME}" -srcfolder "${staging}" -ov -format UDZO "${image}"
  rm -rf "${staging}"
fi

printf "archive=dist/%s\n" "${image}" >> "${GITHUB_ENV:-/dev/null}"
echo "built ${image}"
shasum -a 256 "${image}" > "${image}".sha256
mkdir -p dist
mv "${image}" "${image}.sha256" dist/
rm -rf "${app}"
