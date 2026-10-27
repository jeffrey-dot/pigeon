#!/bin/bash
# Reproducible GUI checklist for M0: real window, real keystrokes, real resize.
#
# Needs Accessibility permission for the terminal that runs this script
# (System Settings → Privacy & Security → Accessibility): keystrokes and window
# resizing go through System Events.
#
#   tools/m0-gui-check.sh
#
# Everything the run needs (demo files, pane scripts) is generated here, so a
# fresh clone can reproduce it. Screenshots land in docs/images/m0/.
set -u
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-docs/images/m0}
mkdir -p "$OUT"

WORK=$(mktemp -d /tmp/pigeon-gui-check-XXXXXX)
trap 'rm -rf "$WORK"' EXIT

# ---- fixtures (generated, not checked in) --------------------------------
seq 1 200 | sed 's/^/line /' > "$WORK/long.txt"
printf 'line one\nline two\n中文 wide chars\n' > "$WORK/demo.txt"
printf '#!/bin/sh\nexec vim -u NONE -n %s\n' "$WORK/demo.txt" > "$WORK/vim.sh"
printf '#!/bin/sh\nexec less %s\n' "$WORK/long.txt" > "$WORK/less.sh"
chmod +x "$WORK/vim.sh" "$WORK/less.sh"

osa() { osascript "$@" >/dev/null 2>&1; }
shoot() { screencapture -x -o "$1" 2>/dev/null; sips -Z 1400 -s format jpeg -s formatOptions 65 "$1" --out "$1" >/dev/null 2>&1; }

start() { # $1 pane program
    R=$(mktemp -d "$WORK/run-XXXXXX")
    PIGEON_DEBUG=1 "$HOST" --socket "$R/pigeon.sock" --shell "$1" >"$R/host.log" 2>&1 &
    HOST_PID=$!
    sleep 1
    XDG_RUNTIME_DIR="$R" "$APP" >"$R/app.log" 2>&1 &
    APP_PID=$!
    sleep 4
    osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
}

children() { pgrep -P "$HOST_PID" 2>/dev/null | wc -l | tr -d ' '; }
stop() { kill "$APP_PID" 2>/dev/null; sleep 1; echo "  pane processes after the window closed: $(children)"; kill "$HOST_PID" 2>/dev/null; }

echo "== typing in the shell =="
start /bin/sh
osa -e 'tell application "System Events" to keystroke "echo typed-by-a-human"'
osa -e 'tell application "System Events" to key code 36'
sleep 2
shoot "$OUT/typed-shell.jpg"
stop

echo "== typing inside vim, then closing and reopening the window =="
PANE=$WORK/vim.sh start "$WORK/vim.sh"
osa -e 'tell application "System Events" to key code 0'      # i
osa -e 'tell application "System Events" to keystroke "alt screen survives"'
osa -e 'tell application "System Events" to key code 53'     # Escape
sleep 1
shoot "$OUT/alt-1.jpg"
kill "$APP_PID" 2>/dev/null; sleep 1
echo "  vim still running after the window closed: $(children)"
XDG_RUNTIME_DIR="$R" "$APP" >/dev/null 2>&1 &
APP_PID=$!
sleep 4
shoot "$OUT/alt-2.jpg"
stop

echo "== paging less with the keyboard =="
start "$WORK/less.sh"
osa -e 'tell application "System Events" to key code 49'     # space
sleep 1
shoot "$OUT/less-page2.jpg"
stop

echo "== htop: render, then quit from the keyboard =="
start /opt/homebrew/bin/htop
shoot "$OUT/htop.jpg"
osa -e 'tell application "System Events" to keystroke "q"'
sleep 2
echo "  htop processes after pressing q: $(children)"
shoot "$OUT/htop-quit.jpg"
stop

echo "== resizing the window =="
start /bin/sh
before=$(grep -c resize_pane "$R/host.log")
osa -e 'tell application "System Events" to tell process "pigeon" to set size of window 1 to {1200, 700}'
sleep 2
after=$(grep -c resize_pane "$R/host.log")
echo "  resize_pane before=$before after=$after"
shoot "$OUT/window-resized.jpg"
stop
