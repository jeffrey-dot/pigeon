#!/bin/bash
# M2 GUI check: the things only a real window can prove — a killed host comes back on its
# own, a second window shows up as a second size owner, and real keystrokes reach the pane
# as `zoom_pane` / `scroll` messages.
#
# The evidence is the host's own log (PIGEON_DEBUG prints every message it receives) plus the
# socket file, so this does not need screenshots. Keyboard steps need Accessibility permission
# for System Events; when that is missing they are reported as SKIP instead of quietly passing.
#
# Build first: `cargo build --release --bin pigeon-host && cargo build && swift build`.
set -u
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
R=$(mktemp -d /tmp/pigeon-m2-XXXXXX)
export XDG_RUNTIME_DIR="$R"
SOCKET="$R/pigeon.sock"
LOG="$R/host.log"
APP_LOG="$R/app.log"
fail=0

note() { printf '%s\n' "$*"; }
pass() { note "PASS  $*"; }
bad() { note "FAIL  $*"; fail=1; }
skip() { note "SKIP  $*"; }

osa_out() { osascript "$@" 2>/dev/null; }
# Failures must be visible: a hidden osascript error once looked like "no Accessibility".
keycode() {
    local out
    out=$(osascript -e "tell application \"System Events\" to key code $1 using {$2}" 2>&1) ||
        { bad "System Events refused the keystroke: $out"; return 1; }
    return 0
}
# The SwiftPM binary runs as "pigeon-bin", so address the process by pid instead of name.
frontmost() {
    local tries=0 pid
    pid=$(pgrep -f "^$APP$" | head -1)
    [ -n "$pid" ] || pid=$(pgrep -f "$APP" | head -1)
    [ -n "$pid" ] || return 1
    while [ "$tries" -lt 10 ]; do
        osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" >/dev/null 2>&1
        sleep 0.3
        if [ "$(osascript -e "tell application \"System Events\" to get frontmost of (first process whose unix id is $pid)" 2>/dev/null)" = "true" ]; then
            return 0
        fi
        tries=$((tries + 1))
    done
    return 1
}
wait_for() { # wait_for <seconds> <command...>
    local deadline=$((SECONDS + $1))
    shift
    while [ "$SECONDS" -lt "$deadline" ]; do
        if "$@" >/dev/null 2>&1; then return 0; fi
        sleep 0.2
    done
    return 1
}

cleanup() {
    pkill -f "$APP" 2>/dev/null
    pkill -f "$HOST" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

cleanup
PIGEON_DEBUG=1 "$HOST" --socket "$SOCKET" --shell /bin/sh >"$LOG" 2>&1 &
HOST_PID=$!
wait_for 5 test -S "$SOCKET" || { bad "host never listened"; exit 1; }

PIGEON_DEBUG=1 "$APP" >"$APP_LOG" 2>&1 &
APP_PID=$!
wait_for 10 grep -q "layout" "$APP_LOG" || bad "the window never got a layout"
pass "window attached to the local host"

# --- one: a second window is a second size owner (before any restart: a restarted host
# starts empty, so both windows would create their own session instead of sharing one) -----
PIGEON_DEBUG=1 "$APP" >"$R/app2.log" 2>&1 &
SECOND=$!
if wait_for 20 grep -q "size_state .*clients=2" "$LOG"; then
    pass "a second window showed up as clients=2 in size_state"
else
    bad "the host never reported two clients"
fi
kill "$SECOND" 2>/dev/null
wait "$SECOND" 2>/dev/null

# --- two: kill -9 the host; the window has to bring it back on its own -------------------
sleep 1
kill -9 "$HOST_PID" 2>/dev/null
# The window restarts the host through HostLauncher, which runs it without our `--socket`
# argument (it uses the default path, which is the same file under XDG_RUNTIME_DIR).
wait_for 20 sh -c "test -S '$SOCKET' && pgrep -f 'pigeon-host' >/dev/null" ||
    bad "the host did not come back after kill -9"
if wait_for 20 grep -q "host unreachable" "$APP_LOG"; then
    pass "kill -9: window noticed the dead host and restarted one"
else
    skip "kill -9: the restart happened, but the window did not log the loss"
fi
HOST_PID=$(pgrep -f 'pigeon-host' | head -1)
# The window restarts the host as its own child, so the new host inherits the app's stderr
# (where PIGEON_DEBUG writes). Everything from here on is read from there.
LOG="$APP_LOG"

# --- two: real keystrokes: split, zoom, unzoom ------------------------------------------
# The menu's zoom item is Cmd-Shift-Return (key code 36), not Cmd-Shift-D: that one is
# "Split Top/Bottom" and would never produce a zoom_pane.
if frontmost; then
    pass "the window came to the front"
else
    bad "the window never became frontmost; the keyboard steps cannot be trusted"
fi
keycode 2 "command down" && wait_for 10 grep -q "<- split_pane" "$LOG" &&
    pass "Cmd-D reached the host as split_pane" ||
    bad "Cmd-D did not produce split_pane"
keycode 36 "command down, shift down" && wait_for 10 grep -q "<- zoom_pane" "$LOG" &&
    pass "Cmd-Shift-Return reached the host as zoom_pane" ||
    bad "Cmd-Shift-Return did not produce zoom_pane"
keycode 36 "command down, shift down" && wait_for 10 grep -q "<- unzoom" "$LOG" &&
    pass "Cmd-Shift-Return again sent unzoom" ||
    bad "the second Cmd-Shift-Return did not produce unzoom"

# --- three: PageUp asks the host to look back ------------------------------------------
keycode 116 "" && wait_for 10 grep -q "<- scroll" "$LOG" &&
    pass "PageUp reached the host as scroll" ||
    bad "PageUp did not produce a scroll message"
keycode 121 "" || true   # PageDown
note "scroll messages recorded in this run: $(grep -c '<- scroll' "$LOG")"

note "--- host log (messages the GUI produced) ---"
grep -E "<- (split_pane|zoom_pane|unzoom|scroll|attach|resize_pane)|size_state" "$LOG" | tail -20
[ "$fail" -eq 0 ] && note "m2-gui-check: no failures" || note "m2-gui-check: FAILURES"
exit "$fail"
