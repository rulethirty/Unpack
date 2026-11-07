#!/bin/sh
# Per-user install only; no sudo and no MIME-default changes.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PREFIX=${PREFIX:-"$HOME/.local"}
DATA=${XDG_DATA_HOME:-"$HOME/.local/share"}
BIN="$ROOT/target/release/unpack"
RECEIPT="$DATA/unpack/program-files.sha256"
test -f "$BIN" || { echo 'Build first: cargo build --release --locked' >&2; exit 1; }
for target in "$RECEIPT" "$PREFIX/bin/unpack" "$DATA/applications/io.github.unpack.Unpack.desktop" "$DATA/metainfo/io.github.unpack.Unpack.metainfo.xml" "$DATA/icons/hicolor/scalable/apps/io.github.unpack.Unpack.svg"; do
    { test ! -e "$target" && test ! -L "$target"; } || { printf 'Refusing to overwrite: %s\n' "$target" >&2; exit 1; }
done
install -Dm755 "$BIN" "$PREFIX/bin/unpack"
install -Dm644 "$ROOT/data/io.github.unpack.Unpack.desktop" "$DATA/applications/io.github.unpack.Unpack.desktop"
install -Dm644 "$ROOT/data/io.github.unpack.Unpack.metainfo.xml" "$DATA/metainfo/io.github.unpack.Unpack.metainfo.xml"
install -Dm644 "$ROOT/data/icons/io.github.unpack.Unpack.svg" "$DATA/icons/hicolor/scalable/apps/io.github.unpack.Unpack.svg"
mkdir -p -- "$DATA/unpack"
sha256sum -- "$PREFIX/bin/unpack" "$DATA/applications/io.github.unpack.Unpack.desktop" \
    "$DATA/metainfo/io.github.unpack.Unpack.metainfo.xml" "$DATA/icons/hicolor/scalable/apps/io.github.unpack.Unpack.svg" > "$RECEIPT"
chmod 600 "$RECEIPT"
if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$DATA/applications"; fi
printf 'Installed Unpack to %s/bin. Ensure this directory is on PATH. No default applications were changed.\n' "$PREFIX"