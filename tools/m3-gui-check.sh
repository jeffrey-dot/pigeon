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
        # The selected pill only paints `WHITE` while the window is key; bring the app
        # forward so the screenshots show the key-window state.
        osa -e 'tell application "System Events" to set frontmost of process "pigeon" to true'
        sleep 0.4
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
# the sidebar header (56) plus its top padding (4) — no 28pt titlebar offset. `OY` is the
# window's screen origin, and clicks are screen coordinates.
ROW_TOP=$((56 + 4))
row_y() {
        python3 -c "
kinds = '$KINDS'
h = {'g': 42, 's': 31}
y = $OY + $ROW_TOP
for k in kinds[:$1]:
    y += h[k]
print(y + h[kinds[$1]] // 2)
"
}

launch() { # launch <light|dark>
        PIGEON_DEBUG=1 PIGEON_APPEARANCE="$1" PIGEON_CONFIG_DIR="$CONFIG" XDG_RUNTIME_DIR="$R" \
                "$APP" >"$R/app-$1.log" 2>&1 &
        APP_PID=$!
        # Give the panes time to get their first snapshot: an empty grid only shows the
        # background, which would hide the terminal's default foreground in the screenshots.
        sleep 6
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
# Second session in the same project sets its own OSC 0 title, so the two rows of group A
# are distinguishable in the before/after drag screenshots.
printf '#!/bin/sh\nprintf "\\033]0;beta\\007"\nexec /bin/sh\n' >"$R/beta.sh"
chmod +x "$R/beta.sh"
cargo run --quiet --manifest-path core/Cargo.toml --example keys -- \
        --socket "$R/pigeon.sock" --new-session /tmp/pigeon/a --shell "$R/beta.sh" >/dev/null 2>&1
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

# Hover a row that is *not* the selection (C group's blocked session): a hovered row that
# is also selected would paint the selected surface, not the hover surface.
cliclick m:150,$(row_y 6)
sleep 1
shoot "$OUT/hover-light.jpg"
cliclick m:150,$((OY + 560))
sleep 0.5

# Hot appearance switch inside the same process: the debug menu item flips NSApp.appearance,
# which is the path AppKit takes when the user changes the system appearance.
key a "command down, option down"
sleep 1
shoot "$OUT/hot-switch-dark.jpg"
key a "command down, option down"
sleep 1
shoot "$OUT/hot-switch-light.jpg"
echo "hot switch: dark + light captured from one process"

before=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
key j "command down"
sleep 1
after=$(grep -c 'pigeon-host: <- focus' "$R/host.log")
echo "Cmd-J focus messages: $before -> $after (ack: $(grep -c 'pigeon-host: <- ack_status' "$R/host.log"))"
shoot "$OUT/jump-light.jpg"

# The menu itself is the visual evidence; closing then goes through ⌘⇧W, which calls the
# same `closeSession` and does not depend on where a synthetic click lands inside a menu.
cliclick rc:100,$(row_y 2) w:600
sleep 1
shoot "$OUT/menu-light.jpg"
cliclick c:600,420
sleep 1
before=$(grep -c 'pigeon-host: <- close_pane' "$R/host.log")
cliclick c:150,$(row_y 6)
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

# Drag reorder is not exercised here: AppKit refuses to start a drag session from
# synthetic mouse events in this environment (the `drag start` debug line never fires, see
# `docs/m3-checklist.md` §5). The ordering *mechanism* is covered by re-opening the app and
# by the manual drag recorded there.
echo "order (manual drag, see docs/m3-checklist.md §5): $(sidebar_state order)"

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
echo "run dir: $R"
echo "screenshots: $(ls "$OUT" | tr '\n' ' ')"
