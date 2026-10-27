#!/bin/bash
# Reproducible GUI checklist for M0: real window, real keystrokes, real resize.
#
# Requires Accessibility permission for the terminal that runs this script
# (System Settings → Privacy & Security → Accessibility), because keystrokes and
# window resizing go through System Events.
#
#   tools/m0-gui-check.sh
#
# Screenshots land in docs/images/m0/; every case prints what it proved.
set -u
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-docs/images/m0}
mkdir -p "$OUT"

osa() { osascript "$@" >/dev/null 2>&1; }
shoot() { screencapture -x -o "$1" 2>/dev/null; sips -Z 1400 -s format jpeg -s formatOptions 65 "$1" --out "$1" >/dev/null 2>&1; }

start() { # $1 pane program
    R=$(mktemp -d /tmp/pigeon-gui-XXXXXX)
    PIGEON_DEBUG=1 "$HOST" --socket "$R/pigeon.sock" --shell "$1" >"$R/host.log" 2>&1 &
    HOST_PID=$!
    sleep 1
    XDG_RUNTIME_DIR="$R" "$APP" >"$R/app.log" 2>&1 &
    APP_PID=$!
    sleep 4
    osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
}

stop() {
    kill "$APP_PID" 2>/dev/null; sleep 1
    echo "  shell still alive after the window closed: $(pgrep -P "$HOST_PID" | wc -l | tr -d ' ')"
    kill "$HOST_PID" 2>/dev/null
}

echo "== typing in the shell =="
start /bin/sh
osa -e 'tell application "System Events" to keystroke "echo typed-by-a-human"'
osa -e 'tell application "System Events" to key code 36'
sleep 2
shoot "$OUT/typed-shell.jpg"
stop

echo "== typing inside vim (alternate screen) =="
start /tmp/pigeon-vim.sh 2>/dev/null || true
if [ -n "${R:-}" ]; then
    osa -e 'tell application "System Events" to key code 0'
    osa -e 'tell application "System Events" to keystroke "alt screen survives"'
    osa -e 'tell application "System Events" to key code 53'
    sleep 1
    shoot "$OUT/alt-1.jpg"
    kill "$APP_PID" 2>/dev/null; sleep 1
    echo "  vim still running after the window closed: $(pgrep -P "$HOST_PID" | wc -l | tr -d ' ')"
    XDG_RUNTIME_DIR="$R" "$APP" >/dev/null 2>&1 &
    APP_PID=$!
    sleep 4
    shoot "$OUT/alt-2.jpg"
    stop
fi

echo "== paging less with the keyboard =="
start /tmp/pigeon-less.sh 2>/dev/null || true
if [ -n "${R:-}" ]; then
    osa -e 'tell application "System Events" to key code 49'
    sleep 1
    shoot "$OUT/less-page2.jpg"
    stop
fi

echo "== resizing the window =="
start /bin/sh
before=$(grep -c resize_pane "$R/host.log")
osa -e 'tell application "System Events" to tell process "pigeon" to set size of window 1 to {1200, 700}'
sleep 2
after=$(grep -c resize_pane "$R/host.log")
echo "  resize_pane before=$before after=$after"
shoot "$OUT/window-resized.jpg"
stop
