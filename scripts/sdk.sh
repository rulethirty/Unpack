#!/bin/sh
# Run from VS Code's Flatpak terminal or a native host shell. Does not install SDKs.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if command -v flatpak >/dev/null 2>&1; then
    exec flatpak run --user --command=sh --devel --filesystem="$ROOT" --share=network \
        --socket=wayland --socket=fallback-x11 --socket=session-bus --share=ipc --device=dri \
        --env=CARGO_HOME="$ROOT/.build/cargo" \
        --env=PATH=/usr/lib/sdk/rust-stable/bin:/usr/bin org.gnome.Sdk//49 \
        -c 'cd "$1"; shift; exec "$@"' sh "$ROOT" "$@"
elif command -v flatpak-spawn >/dev/null 2>&1; then
    exec flatpak-spawn --host sh "$ROOT/scripts/sdk.sh" "$@"
else
    printf '%s\n' 'Flatpak is unavailable. See BUILDING.md for native dependencies.' >&2
    exit 1
fi