#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v flatpak >/dev/null 2>&1; then
    exec flatpak-spawn --host sh "$ROOT/scripts/test-flatpak.sh" "$@"
fi
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
# Isolate test deployment from the user's normal Flatpak app installation.
TEST_ROOT=$(mktemp -d "$ROOT/.build/flatpak-test.XXXXXX")
export FLATPAK_USER_DIR="$TEST_ROOT/installation"
trap 'flatpak uninstall --user --noninteractive -y io.github.unpack.Unpack >/dev/null 2>&1 || true; rm -rf -- "$TEST_ROOT"' EXIT HUP INT TERM
flatpak install --user --noninteractive -y --bundle "$ROOT/dist/Unpack-$VERSION-x86_64.flatpak"
flatpak info --user --show-permissions io.github.unpack.Unpack
flatpak run --user --command=sh io.github.unpack.Unpack -c "$(cat "$ROOT/packaging/flatpak/smoke.sh")"
flatpak run --user io.github.unpack.Unpack --help
flatpak run --user --command=sh io.github.unpack.Unpack -c 'test ! -e /run/host/usr; test "$FLATPAK_ID" = io.github.unpack.Unpack; printf "%s\n" "Sandbox identity and restricted host filesystem verified"'
printf '%s\n' 'FLATPAK PACKAGE PASS'