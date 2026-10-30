#!/bin/bash
# Renderer benchmark for issue #1, on a private host:
#   * `cat` of a large coloured file: wall time until the PTY drained it.
#   * a steady 5 s stream (one screenful every ~8 ms, like a busy build log): the app's CPU
#     time and the frame-time percentiles the app logs (`PIGEON_FRAME_STATS=1`).
# The `cat` runs in the host's PTY, so its wall time is bounded by the host; what differs
# between renderers is how much CPU the app burns keeping up, and how long its frames take.
# Keep the pigeon window in front while it runs: an occluded window's Metal frames are
# throttled by the system (the script warns when that happened).
#
# Build first: cargo build --release --bin pigeon-host && cargo build && swift build
# usage: tools/render-bench.sh [lines]   (default 200000)
set -u
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."

APP=${APP:-.build/debug/pigeon}
HOST=${HOST:-core/target/release/pigeon-host}
LINES=${1:-200000}

R=$(mktemp -d /tmp/pigeon-bench-XXXXXX)
python3 -I -c "
import sys
colours = [31, 32, 33, 34, 35, 36]
with open(sys.argv[1], 'w') as out:
    for i in range(int(sys.argv[2])):
        c = colours[i % 6]
        out.write(f'\x1b[{c}m{i:08d}\x1b[0m the quick brown fox jumps over the lazy dog 中文 \x1b[1mbold\x1b[0m\n')
" "$R/big.txt" "$LINES"

for renderer in coretext metal; do
    # The pane's shell waits until the window is up and attached, runs the cat and the
    # stream, writes the cat's start/end times for this script, then stays open.
    cat >"$R/shell-$renderer.sh" <<SH
#!/bin/sh
sleep 3
start=\$(python3 -c 'import time; print(time.time())')
cat "$R/big.txt"
end=\$(python3 -c 'import time; print(time.time())')
clear
python3 -I -c '
import sys, time
end = time.time() + 5
i = 0
while time.time() < end:
    sys.stdout.write("".join(f"\\x1b[3{i % 6 + 1}m{i:08d}\\x1b[0m steady stream line {n}\\n" for n in range(60)))
    sys.stdout.flush()
    i += 1
    time.sleep(0.008)
'
echo "\$start \$end" >"$R/wall-$renderer"
exec /bin/sh
SH
    chmod +x "$R/shell-$renderer.sh"
    mkdir -p "$R/$renderer"
    "$HOST" --socket "$R/$renderer/pigeon.sock" --shell "$R/shell-$renderer.sh" >"$R/host-$renderer.log" 2>&1 &
    HOST_PID=$!
    sleep 1
    PIGEON_FRAME_STATS=1 PIGEON_RENDERER=$renderer PIGEON_CONFIG_DIR="$R/$renderer/config" \
        PIGEON_HOST_PATH="$HOST" XDG_RUNTIME_DIR="$R/$renderer" \
        "$APP" >"$R/app-$renderer.log" 2>&1 &
    APP_PID=$!
    while [ ! -s "$R/wall-$renderer" ]; do sleep 0.5; done
    # Let the app drain what the host already sent.
    sleep 2
    cpu=$(ps -o time= -p "$APP_PID" | tr -d ' ')
    kill "$APP_PID" 2>/dev/null
    wait "$APP_PID" 2>/dev/null
    kill "$HOST_PID" 2>/dev/null
    wait "$HOST_PID" 2>/dev/null
    wall=$(python3 -c "s, e = open('$R/wall-$renderer').read().split(); print(f'{float(e) - float(s):.2f}')")
    echo "$renderer: cat lines=$LINES wall=${wall}s | app cpu over cat + 5s stream = $cpu"
    if grep -q "window visible=false" "$R/app-$renderer.log" && ! grep -q "window visible=true" "$R/app-$renderer.log"; then
        # macOS throttles the display link of an occluded window to ~1 Hz: the Metal frame
        # count then measures the throttle, not the renderer. Keep the window in front.
        echo "  WARNING: the window was occluded the whole run; frame rates are throttled"
    fi
    python3 -I - "$R/app-$renderer.log" <<'PY'
import re, sys
cpu, gpu, n = [], [], 0
for line in open(sys.argv[1]):
    m = re.search(r'frames renderer=\w+ n=(\d+) cpu_p50=(\d+)us cpu_p99=(\d+)us(?: gpu_p50=(\d+)us gpu_p99=(\d+)us)?', line)
    if not m:
        continue
    n += int(m[1])
    cpu.append(int(m[3]))
    if m[5]:
        gpu.append(int(m[5]))
if n:
    print(f"  frames={n} ({n / 5.0:.0f}/s over the stream) worst cpu_p99={max(cpu)}us" + (f" gpu_p99={max(gpu)}us" if gpu else ""))
else:
    print("  no frame stats logged")
PY
done
echo "logs: $R"
