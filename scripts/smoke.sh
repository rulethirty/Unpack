#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
mkdir -p "$TEMP/config/unpack" "$TEMP/data" "$TEMP/state"
printf '%s\n' '{"schema_version":1,"onboarding_complete":true,"open_behavior":"browser"}' > "$TEMP/config/unpack/settings.json"
mkdir -p "$TEMP/example/bin" "$TEMP/example/lib" "$TEMP/example/resources"
printf '#!/bin/sh\nprintf "Smoke fixture; never executed during inspection\\n"\n' > "$TEMP/example/bin/example"
chmod 755 "$TEMP/example/bin/example"
printf 'Smoke-test README. No commands are executed.\n' > "$TEMP/example/README.md"
printf '[Desktop Entry]\nName=Friendly Example\nIcon=example\nExec=do-not-import\n' > "$TEMP/example/example.desktop"
printf '%s' 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMwbXnyHwAFLAKdz0PptAAAAABJRU5ErkJggg==' | base64 -d > "$TEMP/example/example.png"
tar -czf "$TEMP/example.tar.gz" -C "$TEMP" example
XDG_CONFIG_HOME="$TEMP/config" XDG_DATA_HOME="$TEMP/data" XDG_STATE_HOME="$TEMP/state" \
    UNPACK_SMOKE_TEST=1 G_DEBUG=fatal-criticals "${UNPACK_BINARY:-$ROOT/target/debug/unpack}" --browse "$TEMP/example.tar.gz"