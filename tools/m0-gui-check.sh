#!/bin/bash
# Reproducible GUI checklist for M0. Every step prints what it proved.
#
#   tools/m0-gui-check.sh            # uses .build/debug/pigeon + release host
#
# Screenshots land in docs/images/m0/. Nothing here touches your shell config.
set -u
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-docs/images/m0}
mkdir -p "$OUT"

run_case() {           # $1 pane program, $2 screenshot name, $3 extra wait
    local program=$1 shot=$2 wait=${3:-4}
    local dir; dir=$(mktemp -d /tmp/pigeon-gui-XXXXXX)
    PIGEON_DEBUG=1 "$HOST" --socket "$dir/pigeon.sock" --shell "$program" >"$dir/host.log" 2>&1 &
    local host_pid=$!
    sleep 1
    XDG_RUNTIME_DIR="$dir" PIGEON_DEBUG=1 "$APP" >"$dir/app.log" 2>&1 &
    local app_pid=$!
    sleep "$wait"
    screencapture -x -o "$OUT/$shot.jpg" 2>/dev/null
    echo "[$shot] screenshot: $OUT/$shot.jpg"
    echo "[$shot] resize_pane sent: $(grep -c 'resize_pane' "$dir/host.log")"
    kill "$app_pid" 2>/dev/null; sleep 1
    echo "[$shot] shell alive after the window closed: $(pgrep -P "$host_pid" | wc -l | tr -d ' ')"
    kill "$host_pid" 2>/dev/null
    echo
}

echo "== htop =="
run_case /opt/homebrew/bin/htop htop 5

echo "== close a window, reopen it: the same pane must come back =="
dir=$(mktemp -d /tmp/pigeon-reopen-XXXXXX)
PIGEON_DEBUG=1 "$HOST" --socket "$dir/pigeon.sock" --shell /bin/sh >"$dir/host.log" 2>&1 &
host_pid=$!
sleep 1
XDG_RUNTIME_DIR="$dir" "$APP" >/dev/null 2>&1 &
app1=$!
sleep 4
screencapture -x -o "$OUT/reopen-1.jpg" 2>/dev/null
kill "$app1" 2>/dev/null; sleep 1
echo "after closing the window, the shell is still there: $(pgrep -P "$host_pid" | wc -l | tr -d ' ')"
XDG_RUNTIME_DIR="$dir" "$APP" >/dev/null 2>&1 &
app2=$!
sleep 4
screencapture -x -o "$OUT/reopen-2.jpg" 2>/dev/null
kill "$app2" 2>/dev/null
kill "$host_pid" 2>/dev/null
echo "screenshots: $OUT/reopen-1.jpg $OUT/reopen-2.jpg"
