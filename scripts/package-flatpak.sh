#!/bin/sh
# Run on the host. No flatpak-builder or root privileges are required.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v flatpak >/dev/null 2>&1; then
    exec flatpak-spawn --host sh "$ROOT/scripts/package-flatpak.sh" "$@"
fi
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
ARCH=$(flatpak --default-arch)
test "$ARCH" = x86_64 || { echo 'This packaging configuration currently supports x86_64 only.' >&2; exit 1; }
test -x "$ROOT/target/release/unpack" || { echo 'Build the release binary first.' >&2; exit 1; }
mkdir -p "$ROOT/.build"
WORK=$(mktemp -d "$ROOT/.build/flatpak.XXXXXX")
trap 'rm -rf -- "$WORK"' EXIT HUP INT TERM
STAGE="$WORK/app"
flatpak build-init --arch="$ARCH" "$STAGE" io.github.unpack.Unpack org.gnome.Sdk org.gnome.Platform 49
install -Dm755 "$ROOT/target/release/unpack" "$STAGE/files/bin/unpack"
install -Dm644 "$ROOT/data/io.github.unpack.Unpack.desktop" "$STAGE/files/share/applications/io.github.unpack.Unpack.desktop"
install -Dm644 "$ROOT/packaging/flatpak/io.github.unpack.Unpack.metainfo.xml" "$STAGE/files/share/metainfo/io.github.unpack.Unpack.metainfo.xml"
install -Dm644 "$ROOT/data/icons/io.github.unpack.Unpack.svg" "$STAGE/files/share/icons/hicolor/scalable/apps/io.github.unpack.Unpack.svg"
install -Dm644 "$ROOT/LICENSE" "$STAGE/files/share/licenses/unpack/LICENSE"
# Keep these literal flags in sync with permissions.json and the builder manifest.
flatpak build-finish --command=unpack --share=ipc --socket=wayland --socket=fallback-x11 \
    --device=dri --filesystem=xdg-download --env=UNPACK_PACKAGE=flatpak "$STAGE"
mkdir -p "$ROOT/dist" "$ROOT/.build/flatpak-repo"
flatpak build-export --arch="$ARCH" --subject="Unpack $VERSION development package" \
    "$ROOT/.build/flatpak-repo" "$STAGE" stable
flatpak build-bundle --arch="$ARCH" --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo \
    "$ROOT/.build/flatpak-repo" "$ROOT/dist/Unpack-$VERSION-$ARCH.flatpak" io.github.unpack.Unpack stable
printf 'Created %s/dist/Unpack-%s-%s.flatpak\n' "$ROOT" "$VERSION" "$ARCH"