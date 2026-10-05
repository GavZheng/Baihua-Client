#!/usr/bin/env bash
# AppRun entry point inside an AppDir: it resolves the mounted image directory and
# starts the real binary from there. Copied to <AppDir>/AppRun by the packaging script.
set -eu
here="$(dirname "$(readlink -f "$0")")"
exec "${here}/usr/bin/baihua-gui" "$@"
