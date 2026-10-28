#!/bin/sh
# VT parity: feed one byte stream to the kernel and to a reference terminal (tmux), then
# compare the two screens line by line. See README.md for adding a case.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
cols=${COLS:-80}
rows=${ROWS:-24}

record=0
case "${1:-}" in
"") ;;
--record) record=1 ;;
*)
        echo "usage: $0 [--record]" >&2
        exit 2
        ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Trailing spaces and trailing blank rows carry no meaning on either side; everything else
# (including blank lines in the middle) has to match exactly.
normalize() {
        # tmux writes long blank runs into tabs; expand them on both sides so the comparison is
        # about content, not about how each side encodes blanks.
        expand | awk '{ sub(/[ \t]+$/, ""); rows[NR]=$0 } END {
        last=NR
        while (last > 0 && rows[last] == "") last--
        for (i = 1; i <= last; i++) print rows[i]
    }'
}

if [ "$record" = 1 ] && ! command -v tmux >/dev/null 2>&1; then
        echo "run.sh: --record needs tmux, the reference terminal" >&2
        exit 2
fi

cargo build -q --manifest-path "$root/core/Cargo.toml" --example vt-dump
dump="$root/core/target/debug/examples/vt-dump"

# Records one corpus from tmux. `-f /dev/null` ignores the user's tmux.conf so the pane is
# exactly cols×rows, and `-L` keeps our server away from the user's sessions.
record_tmux() {
        file=$1
        socket="vt-parity-$$"
        tmux -L "$socket" -f /dev/null new-session -d -x "$cols" -y "$rows" \
                "cat '$file'; sleep 5" >/dev/null 2>&1
        tries=0
        while [ "$tries" -lt 50 ]; do
                if tmux -L "$socket" capture-pane -p >/dev/null 2>&1; then break; fi
                tries=$((tries + 1))
                sleep 0.1
        done
        # The pane consumes bytes asynchronously; a short settle keeps the capture repeatable.
        sleep 0.3
        tmux -L "$socket" capture-pane -p
        tmux -L "$socket" kill-server >/dev/null 2>&1 || true
}

failed=0
for corpus in "$here"/corpus/*.vt; do
        name=$(basename "$corpus" .vt)
        golden="$here/golden/$name.txt"
        if [ "$record" = 1 ]; then
                record_tmux "$corpus" | normalize >"$golden"
        fi
        if [ ! -f "$golden" ]; then
                echo "MISSING $name (run $0 --record)" >&2
                failed=1
                continue
        fi
        "$dump" --cols "$cols" --rows "$rows" <"$corpus" | normalize >"$work/$name.txt"
        if diff -u "$golden" "$work/$name.txt" >"$work/$name.diff"; then
                echo "ok      $name"
        else
                echo "FAIL    $name"
                sed 's/^/        /' "$work/$name.diff"
                failed=1
        fi
done

if [ "$failed" -eq 0 ]; then
        echo "vt-parity: all corpora match tmux ${cols}x${rows}"
fi
exit "$failed"
