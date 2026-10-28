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
osa_out() { osascript "$@" 2>/dev/null; }
key() { osa -e "tell application \"System Events\" to keystroke \"$1\" using {$2}"; }
# Window-only captures: `-R` crops to the app's own frame, so the screenshots do not
# carry the rest of the desktop.
shoot() {
        screencapture -x -o -R"$OX,$OY,$W,$H" "$1" 2>/dev/null
        sips -Z 1100 -s format jpeg -s formatOptions 70 "$1" --out "$1" >/dev/null 2>&1
}
read_frame() {
        local frame
        frame=$(osa_out -e 'tell application "System Events" to tell process "pigeon" to get {position, size} of window 1')
        read -r OX OY W H <<<"$(python3 -c "
raw = \"\"\"$frame\"\"\".replace('{', '').replace('}', '').replace(',', ' ')
n = [int(float(x)) for x in raw.split()]
print(n[0], n[1], n[2], n[3])
")"
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
# `.fullSizeContentView` puts the content at the window's top edge, so the list starts at
# the sidebar header (56) plus its top padding (4) — no 28pt titlebar offset.
ROW_TOP=$((56 + 4))
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
        read_frame
        echo "window: origin=($OX,$OY) size=($W,$H)"
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
echo "sidebar width: $(grep -o 'sidebar width=[0-9]*' "$R/app-light.log" | tail -1)"
echo "apps: $(grep -o 'sessions apps=[^"]*' "$R/app-light.log" | tail -1)"

before=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
key j "command down"
sleep 1
after=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
echo "Cmd-J focus messages: $before -> $after (ack: $(grep -c 'pigeon-host: <- ack_status' "$R/host.log"))"
shoot "$OUT/jump-light.jpg"

# Drag session 1 below session 2 inside group A: the drop handler inserts the dragged
# session at the target's index, and the target must be a session in a later group.

# The menu itself is visual evidence; closing goes through ⌘⇧W (same `closeSession`),
# which does not depend on where a synthetic click lands in a menu.
cliclick rc:100,$(row_y 2) w:600
sleep 1
shoot "$OUT/menu-light.jpg"
# Leave the menu without picking anything, then close through ⌘⇧W: both paths call the
# same `closeSession`, and the shortcut does not depend on menu placement.
cliclick c:600,420
sleep 1
before=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
cliclick c:150,$(row_y 2)
sleep 1
echo "selected before close: $(grep -o 'selected=[0-9-]*' "$R/app-light.log" | tail -1)"
osa -e 'tell application "System Events" to keystroke "W" using {command down, shift down}'
sleep 2
after=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
echo "Cmd-Shift-W Close Session: close_pane $before -> $after"

cliclick rc:100,$(row_y 0)
sleep 1
shoot "$OUT/group-menu-light.jpg"
osa -e 'tell application "System Events" to key code 53'
sleep 1

key t "command down"
sleep 2
echo "sessions after Cmd-T: $(grep -c 'pigeon-host: <- new_session' "$R/host.log")"

# Drag session 2 onto session 1 (a real drag has to cross the table's threshold with
# several moves). Dropping on a session inserts at that session's index, so this swaps
# the two rows inside group A.
drag_rows() {
        cliclick dd:100,$(row_y 2)
        sleep 0.3
        cliclick dm:100,$(($(row_y 2) - 14))
        sleep 0.2
        cliclick dm:100,$(row_y 1)
        sleep 0.3
        cliclick du:100,$(row_y 1)
        sleep 1
}
drag_rows
if [ "$(sidebar_state order)" = "[]" ]; then
        echo "drag: first attempt did not register, retrying"
        drag_rows
fi
shoot "$OUT/reordered-light.jpg"
echo "order: $(sidebar_state order)"

# A drag session runs its own modal loop inside AppKit: keep it last, so a stuck drag
# cannot swallow the clicks and keys that produce the rest of the evidence.

kill "$APP_PID" 2>/dev/null
sleep 1
launch light
shoot "$OUT/reopen-light.jpg"
echo "reopened: $(grep -o 'sidebar sessions=[0-9]* needs=[0-9]*' "$R/app-light.log" | tail -1)"
echo "state file: $CONFIG/sidebar.json ($(wc -c <"$CONFIG/sidebar.json" 2>/dev/null || echo 0) bytes)"
kill "$APP_PID" 2>/dev/null
sleep 1

# Hover one row so the hover pill is on the record too.
cliclick m:150,$(row_y 2)
sleep 1
shoot "$OUT/hover-light.jpg"
cliclick m:150,500

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
echo "run dir: $R"
echo "screenshots: $(ls "$OUT" | tr '\n' ' ')"
