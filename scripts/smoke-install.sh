#!/bin/sh
# Install the generated fixture into private workspace/XDG directories, verify
# the Open action and system launcher icon, then remove only the test directory.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$ROOT/.build"
TEST=$(mktemp -d "$ROOT/.build/install-smoke.XXXXXX")
trap 'rm -rf -- "$TEST"' EXIT HUP INT TERM
UNPACK_INSTALL_SMOKE_PARENT="$TEST/apps" sh "$ROOT/scripts/smoke.sh"