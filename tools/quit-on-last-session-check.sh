#!/bin/bash
# End-to-end check: when the last session's shell exits, pigeon closes the session on the
# host and then quits. A private host on a temp socket whose shell exits after a few seconds,
# so nothing touches the user's own sessions and nothing is typed into any window.
#
# Build first: cargo build --release --bin pigeon-host && cargo build && swift build
set -u
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
WAIT=${WAIT:-15}

R=$(mktemp -d /tmp/pigeon-quit-XXXXXX)
cat >"$R/shell.sh" <<'SH'
#!/bin/sh
echo "exiting in 3s"
sleep 3
exit 0
SH
chmod +x "$R/shell.sh"
"$HOST" --socket "$R/pigeon.sock" --shell "$R/shell.sh" >"$R/host.log" 2>&1 &
HOST_PID=$!
sleep 1

PIGEON_DEBUG=1 PIGEON_CONFIG_DIR="$R/config" PIGEON_HOST_PATH="$HOST" XDG_RUNTIME_DIR="$R" \
    "$APP" >"$R/app.log" 2>&1 &
APP_PID=$!

status=1
for _ in $(seq "$WAIT"); do
    sleep 1
    if ! kill -0 "$APP_PID" 2>/dev/null; then
        status=0
        break
    fi
done
if [ "$status" -eq 0 ]; then
    echo "PASS: pigeon quit after the last session's shell exited"
else
    echo "FAIL: pigeon still running ${WAIT}s later"
    kill "$APP_PID" 2>/dev/null
fi
wait "$APP_PID" 2>/dev/null
grep -E "pane exited|last session closed" "$R/app.log"
kill "$HOST_PID" 2>/dev/null
wait "$HOST_PID" 2>/dev/null
echo "logs: $R"
exit "$status"
