#!/bin/bash
# Visual check of the Metal grid renderer (issue #1): one screen with colours, attributes,
# wide glyphs and emoji, one screenshot per appearance, written to $OUT.
# A private host on a temp socket, so nothing touches the user's own sessions.
#
# Build first: cargo build --release --bin pigeon-host && cargo build && swift build
set -u
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-/tmp/pigeon-metal-check}
mkdir -p "$OUT"

R=$(mktemp -d /tmp/pigeon-metal-XXXXXX)
CONFIG="$R/config"
mkdir -p "$CONFIG"
# The pane's shell prints one screen with colours, attributes, wide glyphs and emoji, then
# becomes a plain sh. Nothing is typed through System Events, so no keystroke can land in
# another app's window.
cat >"$R/shell.sh" <<'SH'
#!/bin/sh
printf '\033[1mbold\033[0m \033[3mitalic\033[0m \033[4munder\033[0m \033[9mstrike\033[0m \033[7mrev\033[0m\n'
printf '\033[31mred \033[32mgreen \033[38;5;208m256-orange \033[44;97m blue bg \033[0m\n'
printf '中文字符 wide | emoji 😀 | box ─│┌┐ | done\n'
exec /bin/sh
SH
chmod +x "$R/shell.sh"
"$HOST" --socket "$R/pigeon.sock" --shell "$R/shell.sh" >"$R/host.log" 2>&1 &
HOST_PID=$!
sleep 1

for appearance in light dark; do
    PIGEON_DEBUG=1 PIGEON_APPEARANCE=$appearance \
        PIGEON_CONFIG_DIR="$CONFIG" PIGEON_HOST_PATH="$HOST" XDG_RUNTIME_DIR="$R" \
        "$APP" >"$R/app-$appearance.log" 2>&1 &
    APP_PID=$!
    sleep 4
    osascript -e 'tell application "System Events" to set frontmost of process "pigeon" to true' >/dev/null 2>&1
    sleep 1
    # Capture the window itself, not a screen rectangle: nothing else on screen
    # (a leftover window, the terminal running this script) can end up in the shot.
    WID=$(swift tools/window-id-of-pid.swift "$APP_PID" 2>/dev/null)
    screencapture -x -o -l"$WID" "$OUT/metal-$appearance.png"
    kill "$APP_PID" 2>/dev/null
    wait "$APP_PID" 2>/dev/null
    # One app at a time: the next one must attach as the only client.
    while pgrep -f "$APP" >/dev/null; do sleep 0.2; done
done
grep -h "gpu frame" "$R"/app-*.log | tail -5
kill "$HOST_PID" 2>/dev/null
echo "screenshots: $OUT"
