#!/bin/bash
# Reproducible GUI checklist for M0: a real window, real keystrokes, real resize.
#
# Build first (section 1 of docs/m0-checklist.md). Needs Accessibility permission
# for the terminal that runs this script (System Settings → Privacy & Security →
# Accessibility): keystrokes and window resizing go through System Events.
#
#   tools/m0-gui-check.sh
#
# Every fixture (demo files, pane scripts) is generated in a temp dir, so nothing
# outside the repo is required. Every screenshot below lands in docs/images/m0/.
set -u
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-docs/images/m0}
HTOP=$(command -v htop || true)
mkdir -p "$OUT"

WORK=$(mktemp -d /tmp/pigeon-gui-check-XXXXXX)
trap 'rm -rf "$WORK"' EXIT

seq 1 200 | sed 's/^/line /' > "$WORK/long.txt"
printf 'line one\nline two\n中文 wide chars\n' > "$WORK/demo.txt"
printf '#!/bin/sh\nexec vim -u NONE -n %s\n' "$WORK/demo.txt" > "$WORK/vim.sh"
printf '#!/bin/sh\nexec less %s\n' "$WORK/long.txt" > "$WORK/less.sh"
printf '#!/bin/sh\nprintf "\\033[38;2;255;80;80mTRUECOLOR red\\033[0m \\033[38;2;80;255;80mgreen\\033[0m\\n"\nprintf "中文宽字符 wide 你好世界\\n"\nprintf "\\033[44;37m reverse-ish \\033[0m\\n"\nsleep 30\n' > "$WORK/color.sh"
if [ -n "$HTOP" ]; then
    printf '#!/bin/sh\n%s\nexec /bin/sh\n' "$HTOP" > "$WORK/htop.sh"
fi
chmod +x "$WORK"/*.sh

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

echo "== prompt =="
start /bin/sh
shoot "$OUT/prompt.jpg"
stop

echo "== typing in the shell =="
start /bin/sh
osa -e 'tell application "System Events" to keystroke "echo typed-by-a-human"'
osa -e 'tell application "System Events" to key code 36'
sleep 2
shoot "$OUT/typed-shell.jpg"
stop

echo "== vim first screen =="
start "$WORK/vim.sh"
shoot "$OUT/vim.jpg"
stop

echo "== vim typing, close, reopen =="
start "$WORK/vim.sh"
osa -e 'tell application "System Events" to key code 0'
osa -e 'tell application "System Events" to keystroke "alt screen survives"'
osa -e 'tell application "System Events" to key code 53'
sleep 1
shoot "$OUT/alt-1.jpg"
kill "$APP_PID" 2>/dev/null; sleep 1
echo "  vim still running after the window closed: $(children)"
XDG_RUNTIME_DIR="$R" "$APP" >/dev/null 2>&1 &
APP_PID=$!
sleep 4
osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
# Type again after the reopen: the screenshot must show the new text, which also
# proves the restored pane still accepts input.
osa -e 'tell application "System Events" to key code 0'                     # i
osa -e 'tell application "System Events" to keystroke " after-reopen"'
osa -e 'tell application "System Events" to key code 53'                    # Escape
sleep 1
shoot "$OUT/alt-2.jpg"
stop

echo "== less: first page and one page down =="
start "$WORK/less.sh"
shoot "$OUT/less.jpg"
osa -e 'tell application "System Events" to key code 49'
sleep 1
shoot "$OUT/less-page2.jpg"
stop

if [ -n "$HTOP" ]; then
    echo "== htop: render, quit with q, back to the prompt =="
    start "$WORK/htop.sh"
    shoot "$OUT/htop.jpg"
    osa -e 'tell application "System Events" to keystroke "q"'
    sleep 2
    echo "  htop processes after pressing q: $(children)"
    shoot "$OUT/htop-quit.jpg"
    stop
else
    echo "== htop == skipped (htop not installed)"
fi

echo "== truecolor and wide characters =="
start "$WORK/color.sh"
shoot "$OUT/truecolor.jpg"
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
