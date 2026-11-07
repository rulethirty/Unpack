#!/bin/sh
# Run inside the GNOME SDK, not on an arbitrary host with a different GTK ABI.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
test "${FLATPAK_ID:-}" = org.gnome.Sdk || { printf '%s\n' 'Run: sh scripts/sdk.sh sh scripts/package-appimage.sh' >&2; exit 1; }
python3 "$ROOT/packaging/appimage/build.py"