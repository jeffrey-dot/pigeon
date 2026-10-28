#!/bin/bash
# Reproducible M1 GUI checklist: splits, focus, divider drag, close/reopen.
#
# Needs Accessibility permission for the terminal that runs this script
# (System Events + cliclick drive real input). Build first:
#   cargo build --release --bin pigeon-host && cargo build && swift build
set -u
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-docs/images/m1}
mkdir -p "$OUT"

osa() { osascript "$@" >/dev/null 2>&1; }
key() { osa -e "tell application \"System Events\" to keystroke \"$1\" using {$2}"; }
shift_cmd() { osa -e "tell application \"System Events\" to keystroke \"$1\" using {command down, shift down}"; }
shoot() { screencapture -x -o "$1" 2>/dev/null; sips -Z 1400 -s format jpeg -s formatOptions 65 "$1" --out "$1" >/dev/null 2>&1; }

R=$(mktemp -d /tmp/pigeon-m1-XXXXXX)
PIGEON_DEBUG=1 "$HOST" --socket "$R/pigeon.sock" --shell /bin/sh >"$R/host.log" 2>&1 &
HOST_PID=$!
sleep 1
PIGEON_DEBUG=1 XDG_RUNTIME_DIR="$R" "$APP" >"$R/app.log" 2>&1 &
APP_PID=$!
sleep 4

# Deterministic coordinates for clicks and drags.
osa -e 'tell application "System Events" to tell process "pigeon" to set position of window 1 to {0, 0}'
osa -e 'tell application "System Events" to tell process "pigeon" to set size of window 1 to {900, 560}'
osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
sleep 1

panes() { pgrep -P "$HOST_PID" | wc -l | tr -d ' '; }
echo "panes before splitting: $(panes)"

key d "command down"; sleep 1
shoot "$OUT/split-2.jpg"
echo "after Cmd-D: $(panes) shells, $(grep -c split_pane "$R/host.log") split_pane sent"

key d "command down"; sleep 1
shoot "$OUT/split-3.jpg"
echo "after second Cmd-D: $(panes) shells"

shift_cmd d; sleep 1
shoot "$OUT/split-4.jpg"
echo "after Cmd-Shift-D: $(panes) shells"

# Click the left pane: focus must move there.
cliclick c:200,328
sleep 1
echo "focus messages: $(grep -c focus "$R/host.log")"

# Drag the first vertical divider from x=450 to x=600 (title bar is 28px).
cliclick dd:450,328 dm:560,328 du:600,328
sleep 1
shoot "$OUT/dragged.jpg"
echo "resize_split messages: $(grep -c resize_split "$R/host.log")"

# Close the window: every shell keeps running.
kill "$APP_PID" 2>/dev/null; sleep 1
echo "shells after the window closed: $(panes)"
PIGEON_DEBUG=1 XDG_RUNTIME_DIR="$R" "$APP" >"$R/app2.log" 2>&1 &
APP_PID=$!
sleep 4
osa -e 'tell application "System Events" to tell process "pigeon" to set position of window 1 to {0, 0}'
sleep 1
shoot "$OUT/reopen.jpg"
restored=$(grep -o "pigeon: layout [0-9]* panes" "$R/app2.log" | awk '{print $3}' | sort -n | tail -1)
echo "reopened window: ${restored:-0} panes restored"
kill "$APP_PID" 2>/dev/null
kill "$HOST_PID" 2>/dev/null
echo "screenshots in $OUT: $(ls "$OUT" | tr '\n' ' ')"
