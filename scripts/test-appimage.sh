#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
APPIMAGE="$ROOT/dist/Unpack-$VERSION-x86_64.AppImage"
test -x "$APPIMAGE"
APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE" --help
UNPACK_BINARY="$APPIMAGE" APPIMAGE_EXTRACT_AND_RUN=1 sh "$ROOT/scripts/smoke.sh"
printf '%s\n' 'APPIMAGE PACKAGE PASS'