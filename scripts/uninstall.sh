#!/bin/sh
set -eu
PREFIX=${PREFIX:-"$HOME/.local"}
DATA=${XDG_DATA_HOME:-"$HOME/.local/share"}
RECEIPT="$DATA/unpack/program-files.sha256"
test -f "$RECEIPT" || { printf '%s\n' 'No program ownership receipt found. Refusing to remove untracked files.' >&2; exit 1; }
EXPECTED=$(mktemp)
trap 'rm -f -- "$EXPECTED"' EXIT HUP INT TERM
sha256sum -- "$PREFIX/bin/unpack" "$DATA/applications/io.github.unpack.Unpack.desktop" \
    "$DATA/metainfo/io.github.unpack.Unpack.metainfo.xml" "$DATA/icons/hicolor/scalable/apps/io.github.unpack.Unpack.svg" > "$EXPECTED"
cmp -s -- "$RECEIPT" "$EXPECTED" || { printf '%s\n' 'Program files changed or installation paths differ from the ownership receipt. Nothing was removed.' >&2; exit 1; }
printf '%s\n' 'Removing the Unpack program and its desktop metadata only. Managed applications, manifests, preferences and backups are preserved.'
rm -f -- "$PREFIX/bin/unpack" "$DATA/applications/io.github.unpack.Unpack.desktop" \
    "$DATA/metainfo/io.github.unpack.Unpack.metainfo.xml" "$DATA/icons/hicolor/scalable/apps/io.github.unpack.Unpack.svg"
rm -- "$RECEIPT"
if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$DATA/applications"; fi