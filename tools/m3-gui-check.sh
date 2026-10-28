#!/bin/bash
# Reproducible M3 sidebar check: three project sessions, a status event, Cmd-J,
# Cmd-T, right-click menu, collapse, drag reorder, then reopen.
#
# Needs Accessibility permission (System Events + cliclick). Build first:
#   cargo build --release --bin pigeon-host && cargo build && swift build
set -u
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
OUT=${OUT:-docs/images/m3}
mkdir -p "$OUT"
osa() { osascript "$@" >/dev/null 2>&1; }
key() { osa -e "tell application \"System Events\" to keystroke \"$1\" using {$2}"; }
shoot() { screencapture -x -o "$1" 2>/dev/null; sips -Z 1400 -s format jpeg -s formatOptions 65 "$1" --out "$1" >/dev/null 2>&1; }
sidebar_state() { python3 -c "import json,sys;d=json.load(open('$CONFIG/sidebar.json'));print(d.get('$1'))" 2>/dev/null || echo '?'; }

R=$(mktemp -d /tmp/pigeon-m3-XXXXXX)
CONFIG="$R/config"
mkdir -p "$CONFIG" /tmp/pigeon/a /tmp/pigeon/b /tmp/pigeon/c
PIGEON_DEBUG=1 "$HOST" --socket "$R/pigeon.sock" --shell /bin/sh >"$R/host.log" 2>&1 &
HOST_PID=$!
sleep 1
for project in a b; do
    cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
        --socket "$R/pigeon.sock" --new-session "/tmp/pigeon/$project" >/dev/null 2>&1
done
# Third session reports its own status, so the sidebar has something to show.
printf '#!/bin/sh\nprintf "\\033]7501;state=blocked:kind=permission\\007"\nexec /bin/sh\n' > "$R/status.sh"
chmod +x "$R/status.sh"
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
    --socket "$R/pigeon.sock" --new-session "/tmp/pigeon/c" --shell "$R/status.sh" >/dev/null 2>&1
echo "sessions created: $(grep -c 'pigeon-host: <- new_session' "$R/host.log")"

PIGEON_DEBUG=1 PIGEON_CONFIG_DIR="$CONFIG" XDG_RUNTIME_DIR="$R" "$APP" >"$R/app.log" 2>&1 &
APP_PID=$!
sleep 4
osa -e 'tell application "System Events" to tell process "pigeon" to set position of window 1 to {0, 0}'
osa -e 'tell application "System Events" to tell process "pigeon" to set size of window 1 to {1100, 600}'
osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
sleep 1
# Click coordinates are derived from the real window frame, not assumed.
FRAME=$(osascript -e 'tell application "System Events" to tell process "pigeon" to get {position, size} of window 1' 2>/dev/null)
read -r OX OY W H <<<"$(python3 -c "
import sys
raw='$FRAME'.replace('{','').replace('}','').replace(',',' ')
n=[int(float(x)) for x in raw.split()]
print(n[0], n[1], n[2], n[3])
")"
echo "window: origin=($OX,$OY) size=($W,$H)"
ROW=46; TOP=$((OY + 28))
row_y() { echo $((TOP + ROW * $1 + ROW / 2)); }
shoot "$OUT/sidebar.jpg"
echo "sidebar: $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app.log" | tail -1)"

# The third session (its first pane id equals the session id) reports blocked.
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
    --socket "$R/pigeon.sock" --pane 3 --send "printf '\033]7501;state=blocked\007'" >/dev/null 2>&1
sleep 2
shoot "$OUT/status.jpg"
echo "after blocked: $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app.log" | tail -1)"

before=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
key j "command down"; sleep 1
after=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
echo "Cmd-J focus messages: $before -> $after (ack: $(grep -c 'pigeon-host: <- ack_status' "$R/host.log"))"
shoot "$OUT/jump.jpg"

key t "command down"; sleep 2
echo "sessions after Cmd-T: $(grep -c 'pigeon-host: <- new_session' "$R/host.log")"

# Right-click a session: the first menu item is "Close Session".
before=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
cliclick rc:100,$(row_y 3)
sleep 1
shoot "$OUT/menu.jpg"
osascript -e 'tell application "System Events" to key code 125' >/dev/null 2>&1   # Down
osascript -e 'tell application "System Events" to key code 36' >/dev/null 2>&1    # Return
sleep 2
after=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
echo "right-click Close Session: close_pane $before -> $after"

cliclick c:60,$(row_y 0)
sleep 1
shoot "$OUT/collapsed.jpg"
echo "collapsed: $(sidebar_state collapsed)"

# A real drag has to cross the table's dragging threshold with several moves.
cliclick dd:100,$(row_y 3)
sleep 0.3
cliclick dm:100,$(( $(row_y 3) + 12 ))
sleep 0.2
cliclick dm:100,$(row_y 4)
sleep 0.2
cliclick dm:100,$(row_y 5)
sleep 0.2
cliclick dm:100,$(row_y 6)
sleep 0.3
cliclick du:100,$(row_y 6)
sleep 1
shoot "$OUT/reordered.jpg"
echo "order: $(sidebar_state order)"

kill "$APP_PID" 2>/dev/null; sleep 1
PIGEON_DEBUG=1 PIGEON_CONFIG_DIR="$CONFIG" XDG_RUNTIME_DIR="$R" "$APP" >"$R/app2.log" 2>&1 &
APP_PID=$!
sleep 4
osa -e 'tell application "System Events" to tell process "pigeon" to set position of window 1 to {0, 0}'
sleep 1
shoot "$OUT/reopen.jpg"
echo "reopened: $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app2.log" | tail -1)"
echo "state file: $CONFIG/sidebar.json ($(wc -c < "$CONFIG/sidebar.json" 2>/dev/null || echo 0) bytes)"
kill "$APP_PID" 2>/dev/null; kill "$HOST_PID" 2>/dev/null
echo "screenshots: $(ls "$OUT" | tr '\n' ' ')"
