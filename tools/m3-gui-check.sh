#!/bin/bash
# Reproducible M3 sidebar check against harp's geometry (`docs/ui.md` §7), in both
# appearances: empty state, three projects (one with two sessions for the drag), a done
# session that stays unread, a blocked one, Cmd-J, drag reorder, context menus, reopen.
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
shoot() {
        screencapture -x -o "$1" 2>/dev/null
        sips -Z 1400 -s format jpeg -s formatOptions 65 "$1" --out "$1" >/dev/null 2>&1
}
sidebar_state() { python3 -c "import json,sys;d=json.load(open('$CONFIG/sidebar.json'));print(d.get('$1'))" 2>/dev/null || echo '?'; }

pkill -f "$APP" 2>/dev/null
R=$(mktemp -d /tmp/pigeon-m3-XXXXXX)
CONFIG="$R/config"
mkdir -p "$CONFIG" /tmp/pigeon/a /tmp/pigeon/b /tmp/pigeon/c
PIGEON_DEBUG=1 "$HOST" --socket "$R/pigeon.sock" --shell /bin/sh >"$R/host.log" 2>&1 &
HOST_PID=$!
sleep 1

# Row geometry from `docs/ui.md` §7.1: 56pt sidebar header (traffic lights), 4pt list top
# padding, a 42pt group row (28 header + 14 section gap), a 31pt session row (30 + 1 gap).
# `KINDS` is the row list: group `g`, session `s`.
KINDS="gssgsgs"
ROW_TOP=$((28 + 56 + 4)) # window titlebar is inside the window (fullSizeContentView): 28+56+4
row_y() {
        python3 -c "
kinds = '$KINDS'
h = {'g': 42, 's': 31}
y = $ROW_TOP
for k in kinds[:$1]:
    y += h[k]
print(y + h[kinds[$1]] // 2)
"
}

launch() { # launch <light|dark>
        PIGEON_DEBUG=1 PIGEON_APPEARANCE="$1" PIGEON_CONFIG_DIR="$CONFIG" XDG_RUNTIME_DIR="$R" \
                "$APP" >"$R/app-$1.log" 2>&1 &
        APP_PID=$!
        sleep 4
        osa -e 'tell application "System Events" to tell process "pigeon" to set position of window 1 to {0, 0}'
        osa -e 'tell application "System Events" to tell process "pigeon" to set size of window 1 to {1100, 600}'
        osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
        sleep 1
}

# --- empty state, before any session exists (both appearances) -------------------------
launch light
shoot "$OUT/empty-light.jpg"
kill "$APP_PID" 2>/dev/null
sleep 1
launch dark
shoot "$OUT/empty-dark.jpg"
kill "$APP_PID" 2>/dev/null
sleep 1

# --- sessions -------------------------------------------------------------------------
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
        --socket "$R/pigeon.sock" --new-session /tmp/pigeon/a >/dev/null 2>&1
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
        --socket "$R/pigeon.sock" --new-session /tmp/pigeon/a >/dev/null 2>&1
# A done session that nobody is looking at: unread dot + filled check.
printf '#!/bin/sh\nsleep 2\nprintf "\\033]7501;state=done\\007"\nexec /bin/sh\n' >"$R/done.sh"
chmod +x "$R/done.sh"
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
        --socket "$R/pigeon.sock" --new-session /tmp/pigeon/b --shell "$R/done.sh" >/dev/null 2>&1
# A blocked session: the group header carries `1 needs you`.
printf '#!/bin/sh\nprintf "\\033]7501;state=blocked:kind=permission\\007"\nexec /bin/sh\n' >"$R/blocked.sh"
chmod +x "$R/blocked.sh"
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
        --socket "$R/pigeon.sock" --new-session /tmp/pigeon/c --shell "$R/blocked.sh" >/dev/null 2>&1
echo "sessions created: $(grep -c 'pigeon-host: <- new_session' "$R/host.log")"

# --- light: full interaction pass ------------------------------------------------------
launch light
shoot "$OUT/sidebar-light.jpg"
echo "sidebar (light): $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app-light.log" | tail -1)"
echo "apps: $(grep -o 'sessions apps=[^"]*' "$R/app-light.log" | tail -1)"

before=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
key j "command down"
sleep 1
after=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
echo "Cmd-J focus messages: $before -> $after (ack: $(grep -c 'pigeon-host: <- ack_status' "$R/host.log"))"
shoot "$OUT/jump-light.jpg"

# Drag session 1 below session 2 inside group A: the drop handler inserts the dragged
# session at the target's index, and the target must be a session in a later group.
cliclick dd:100,$(row_y 1)
sleep 0.3
cliclick dm:100,$(($(row_y 1) + 14))
sleep 0.2
cliclick dm:100,$(row_y 2)
sleep 0.2
cliclick dm:100,$(row_y 4)
sleep 0.2
cliclick dm:100,$(row_y 6)
sleep 0.3
cliclick du:100,$(row_y 6)
sleep 1
shoot "$OUT/reordered-light.jpg"
echo "order: $(sidebar_state order)"

before=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
cliclick rc:100,$(row_y 2)
sleep 1
shoot "$OUT/menu-light.jpg"
osa -e 'tell application "System Events" to key code 125' # Down
osa -e 'tell application "System Events" to key code 36'  # Return
sleep 2
after=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
echo "right-click Close Session: close_pane $before -> $after"

cliclick rc:100,$(row_y 0)
sleep 1
shoot "$OUT/group-menu-light.jpg"
osa -e 'tell application "System Events" to key code 53'
sleep 1

key t "command down"
sleep 2
echo "sessions after Cmd-T: $(grep -c 'pigeon-host: <- new_session' "$R/host.log")"

kill "$APP_PID" 2>/dev/null
sleep 1
launch light
shoot "$OUT/reopen-light.jpg"
echo "reopened: $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app-light.log" | tail -1)"
echo "state file: $CONFIG/sidebar.json ($(wc -c <"$CONFIG/sidebar.json" 2>/dev/null || echo 0) bytes)"
kill "$APP_PID" 2>/dev/null
sleep 1

# --- dark: the same list, for the palette half of the comparison table -----------------
launch dark
shoot "$OUT/sidebar-dark.jpg"
key j "command down"
sleep 1
shoot "$OUT/jump-dark.jpg"
echo "sidebar (dark): $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app-dark.log" | tail -1)"

kill "$APP_PID" 2>/dev/null
kill "$HOST_PID" 2>/dev/null
echo "titles: $(grep -o 'sessions apps=[^"]*' "$R/app-light.log" | tail -1)"
echo "screenshots: $(ls "$OUT" | tr '\n' ' ')"
