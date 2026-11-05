#!/usr/bin/env python3
"""Isolated GTK scroll acceptance against a real host; production API records wire evidence.

Build pigeon and pigeon-server first. Requires Xvfb, xdotool, PyGObject and a Rust toolchain.
PIGEON_SCROLL_ARTIFACTS optionally preserves the wire log and screenshots outside the sandbox.
"""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("input_check", Path(__file__).with_name("linux-input-check.py"))
input_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(input_check)
run = input_check.run


def build_proxy():
    with tempfile.TemporaryDirectory(prefix="pigeon-scroll-build-") as directory:
        manifest = Path(directory) / "Cargo.toml"
        manifest.write_text(f'''[package]
name = "pigeon-scroll-proxy"
version = "0.0.0"
edition = "2021"
license = "GPL-3.0-or-later"
[dependencies]
pigeon-core = {{ path = "{ROOT / 'core'}", default-features = false }}
[[bin]]
name = "pigeon-scroll-proxy"
path = "{ROOT / 'tools/checks/helpers/linux-scroll-proxy.rs'}"
''')
        subprocess.run(["cargo", "build", "--offline", "--quiet", "--manifest-path", str(manifest),
                        "--target-dir", str(ROOT / "core/target")], check=True)


def check():
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, Gdk, GLib
    Gtk.init()
    clipboard = Gdk.Display.get_default().get_clipboard()
    temporary = Path(os.environ["TEST_ROOT"])
    children, logs = [], []
    artifacts = os.environ.get("PIGEON_SCROLL_ARTIFACTS")
    if artifacts:
        Path(artifacts).mkdir(parents=True, exist_ok=True)

    def spawn(name, args, **kwargs):
        log = open(temporary / f"{name}.log", "w+")
        logs.append(log)
        child = subprocess.Popen(args, stdout=log, stderr=log, cwd=temporary, **kwargs)
        children.append(child)
        return child

    def pump(predicate, timeout=8):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            while GLib.MainContext.default().iteration(False):
                pass
            if predicate():
                return
            time.sleep(0.01)
        raise AssertionError("timed out waiting for scroll/clipboard evidence")

    def settle(seconds=0.15):
        deadline = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= deadline, timeout=seconds + 2)

    wire = temporary / "wire.log"

    def messages():
        return wire.read_text().splitlines() if wire.exists() else []

    def snapshots(pane=1):
        return [line.split(" ", 10) for line in messages() if line.startswith(f"S snapshot {pane} ")]

    def offset(expected=None, pane=1):
        answers = [parts for parts in snapshots(pane) if int(parts[6]) >= 0]
        if not answers:
            return False
        return expected is None or int(answers[-1][6]) == expected

    def key(*keys):
        run("xdotool", "key", "--clearmodifiers", *keys)
        settle()

    def copy():
        key("ctrl+shift+c")
        result = []
        clipboard.read_text_async(None, lambda clip, task: result.append(clip.read_text_finish(task)))
        pump(lambda: bool(result))
        return result[0]

    def shown():
        settle(0.15)
        text = copy()
        latest = snapshots()[-1]
        assert text.splitlines()[0] == latest[9].rstrip(), (text[:100], latest)
        return text

    def command(text):
        (temporary / "output").write_text(text)
        settle(0.25)

    try:
        config = temporary / "config/fcitx5"
        config.mkdir()
        (config / "profile").write_text("""[Groups/0]
Name=Test
Default Layout=us
DefaultIM=pinyin
[Groups/0/Items/0]
Name=keyboard-us
Layout=
[Groups/0/Items/1]
Name=pinyin
Layout=
[GroupOrder]
0=Test
""")
        spawn("fcitx", ["fcitx5", "-D", "--disable=wayland,waylandim,notificationitem,notifications,cloudpinyin"])
        pump(lambda: subprocess.call(["fcitx5-remote", "--check"], stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL) == 0)
        run("fcitx5-remote", "-c")
        capture = temporary / "capture.py"
        capture.write_text('''#!/usr/bin/python3
import os, select, tty
from pathlib import Path
tty.setraw(0)
root = Path(os.environ['TEST_ROOT'])
path = root / ('bytes-' + str(os.getpid()))
path.touch()
os.write(1, ''.join(f'LINE-{i:05d}\\r\\n' for i in range(11050)).encode())
last = None
while True:
    if select.select([0], [], [], 0.01)[0]:
        data = os.read(0, 4096)
        if not data: break
        with path.open('ab') as output: output.write(data)
    trigger = root / 'output'
    if trigger.exists() and (stamp := trigger.stat().st_mtime_ns) != last:
        last = stamp
        os.write(1, trigger.read_bytes())
''')
        capture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        host_socket = temporary / "runtime/real.sock"
        proxy_socket = temporary / "runtime/pigeon.sock"
        host_env = os.environ.copy()
        host_env.pop("PIGEON_IDLE_MS", None)
        spawn("host", [str(server), "--socket", str(host_socket), "--shell", str(capture)], env=host_env)
        pump(host_socket.exists)
        spawn("proxy", [str(ROOT / "core/target/debug/pigeon-scroll-proxy"), str(proxy_socket), str(host_socket), str(wire)])
        pump(proxy_socket.exists)
        wrapper = temporary / "server"
        wrapper.write_text(f'#!/bin/sh\nprintf "%s\\n" "{proxy_socket}"\n')
        wrapper.chmod(0o700)
        app_env = os.environ.copy()
        app_env.update(PIGEON_SERVER=str(wrapper), GTK_IM_MODULE="fcitx")
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")], env=app_env)
        pump(lambda: bool(list(temporary.glob("bytes-*"))))
        pane = next(temporary.glob("bytes-*"))
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        settle(0.5)
        key("ctrl+shift+b")
        run("xdotool", "windowsize", window, "640", "408")
        settle(0.3)
        assert any(line.startswith("C hello delta,scrollback") for line in messages())
        pump(lambda: any(line.startswith("C resize 1 ") for line in messages()))
        settle(0.3)
        if os.environ.get("PIGEON_TEST_NO_SCROLLBACK"):
            assert any(line.startswith("S hello delta,") and "scrollback" not in line for line in messages())
            key("shift+Page_Up", "shift+End")
            run("xdotool", "mousemove", "--window", window, "100", "100", "click", "4")
            settle()
            assert not any(line.startswith("C scroll") for line in messages())
            assert pane.read_bytes() == b"\x1b[5;2~\x1b[1;2F", pane.read_bytes()
            print("PASS missing scrollback capability disables local page/wheel interception", flush=True)
            return
        assert any(line.startswith("S hello delta,scrollback") for line in messages())
        pump(lambda: any(line.startswith("C resize 1 ") for line in messages()))
        settle(0.3)
        assert "LINE-11049" in copy()
        key("Page_Up", "ctrl+Page_Up", "alt+Page_Down")
        assert pane.read_bytes() == b"\x1b[5~\x1b[5;5~\x1b[6;3~", pane.read_bytes()
        before = pane.read_bytes()
        print("PASS negotiated scrollback; bare/modified live page keys reach the program", flush=True)

        key("shift+Page_Up")
        pump(lambda: offset(23))
        history = shown()
        assert "LINE-11049" not in history and pane.read_bytes() == before
        assert int(snapshots()[-1][7]) == 10000
        command("LIVE-UPDATE\r\n")
        assert copy() == history, "live output tore the history screen"
        if artifacts:
            run("import", "-window", window, str(Path(artifacts) / "history.png"))
        print("PASS 10000-row host history, exact copied slice and frozen history during output", flush=True)
        key("shift+Home")
        pump(lambda: offset(10000))
        oldest = shown()
        key("shift+Home")
        pump(lambda: offset(10000))
        assert shown() == oldest
        key("shift+End", "shift+Page_Up")
        pump(lambda: offset(23))
        print("PASS oldest-history jump clamps at 10000; repeated jump is stable", flush=True)
        if os.environ.get("PIGEON_SCROLL_IDLE_CHECK"):
            frozen = copy()
            settle(47)
            assert copy() == frozen
            assert len([line for line in messages() if line.startswith("S pong")]) >= 3
            key("shift+End")
            pump(lambda: offset(0))
            command("LIVE-AFTER-IDLE\r\n")
            assert "LIVE-AFTER-IDLE" in copy()
            key("shift+Page_Up")
            pump(lambda: offset(23))
            print("PASS 47s idle history reading survives default host timeout; ping/pong and live output resume", flush=True)

        run("xdotool", "key", "--clearmodifiers", "--delay", "1", "Page_Up", "Page_Up", "Page_Down", "shift+End")
        pump(lambda: offset(0))
        settle(0.3)
        assert "LIVE-UPDATE" in copy()
        scrolls = [line for line in messages() if line.startswith("C scroll")]
        assert scrolls[-4:] == ["C scroll 1 -23", "C scroll 1 -23", "C scroll 1 23", "C scroll 1 0"], scrolls
        command("DELTA-AFTER-BOTTOM\r\n")
        assert "DELTA-AFTER-BOTTOM" in copy()
        print("PASS rapid relative requests with delayed replies; bottom and subsequent delta resume", flush=True)

        run("xdotool", "mousemove", "--window", window, "100", "100", "click", "--repeat", "4", "--delay", "5", "4")
        pump(lambda: offset(12))
        shown()
        key("x")
        pump(lambda: offset(0))
        assert pane.read_bytes() == before + b"x"
        key("shift+Page_Up")
        clipboard.set("PASTE")
        key("ctrl+shift+v")
        pump(lambda: offset(0))
        pump(lambda: pane.read_bytes() == before + b"xPASTE")
        print("PASS wheel accumulation, typing/paste return live without losing bytes", flush=True)
        key("shift+Page_Up")
        pump(lambda: offset(23))
        settle()
        run("xdotool", "mousemove", "--window", window, "570", "15", "click", "1")
        pump(lambda: offset(0))
        print("PASS Return to live button", flush=True)
        key("shift+Page_Up")
        pump(lambda: offset(23))
        before = pane.read_bytes()
        run("fcitx5-remote", "-o")
        run("fcitx5-remote", "-s", "pinyin")
        key("n", "i", "h", "a", "o")
        pump(lambda: offset(0))
        assert pane.read_bytes() == before
        key("space")
        pump(lambda: pane.read_bytes() == before + "你好".encode())
        run("fcitx5-remote", "-c")
        print("PASS IME preedit returns live without bytes and commits 你好 once", flush=True)

        key("shift+Page_Up")
        run("xdotool", "windowsize", window, "720", "450")
        pump(lambda: offset(0))
        settle(0.4)
        command("RESIZED-LIVE\r\n")
        assert "RESIZED-LIVE" in copy()
        print("PASS resize returns live and resumes a correctly sized snapshot/delta", flush=True)

        command("\x1b[?1049h\x1b[?1h\x1b[?1007hALT-SCREEN")
        pump(lambda: "ALT-SCREEN" in copy())
        count = len([line for line in messages() if line.startswith("C scroll")])
        before = pane.read_bytes()
        key("Page_Up", "shift+Page_Down")
        run("xdotool", "click", "4")
        settle()
        assert pane.read_bytes() == before + b"\x1b[5~\x1b[6;2~" + b"\x1bOA" * 3, pane.read_bytes()
        assert len([line for line in messages() if line.startswith("C scroll")]) == count
        command("\x1b[?1007l")
        before = pane.read_bytes()
        run("xdotool", "click", "4")
        settle()
        assert pane.read_bytes() == before
        command("\x1b[?1007h\x1b[?1000h")
        run("xdotool", "click", "4")
        settle()
        assert pane.read_bytes() == before, "mouse-reporting apps must not receive fake arrows"
        command("\x1b[?1000l\x1b[?1049l\x1b[?1l")
        print("PASS alternate screen forwards page keys and mode-controlled wheel arrows; no history requests", flush=True)

        key("ctrl+shift+d")
        pump(lambda: len(list(temporary.glob("bytes-*"))) == 2)
        settle(0.4)
        other = next(path for path in temporary.glob("bytes-*") if path != pane)
        key("shift+Page_Up")
        pump(lambda: offset(pane=2))
        other_history = copy()
        run("xdotool", "mousemove", "--window", window, "40", "80", "click", "1")
        key("q")
        assert other.read_bytes() == b""
        assert pane.read_bytes().endswith(b"q")
        run("xdotool", "mousemove", "--window", window, "680", "80", "click", "1")
        assert copy() == other_history
        key("ctrl+shift+z")
        pump(lambda: offset(0, pane=2))
        key("ctrl+shift+z", "ctrl+shift+w")
        print("PASS pane-local scroll/focus/copy; zoom resize returns live; close retains live target", flush=True)
        assert app.poll() is None
        for log in logs:
            log.flush()
        client_log = (temporary / "client.log").read_text()
        assert "panicked" not in client_log and "CRITICAL" not in client_log, client_log
        print("PASS no GTK critical or Rust panic", flush=True)
    finally:
        for child in reversed(children):
            child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        if artifacts:
            for name in ("wire.log", "client.log", "host.log"):
                if (temporary / name).exists():
                    shutil.copy2(temporary / name, Path(artifacts) / name)
        if sys.exc_info()[0]:
            for log in logs:
                log.flush()
                log.seek(0)
                print(f"--- {log.name} ---\n{log.read()[-3000:]}", file=sys.stderr)
            print("\n".join(messages()[-50:]), file=sys.stderr)
        for log in logs:
            log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv:
        check()
    else:
        build_proxy()
        # Reuse the established isolated Xvfb / HOME / D-Bus lifecycle, running this check.
        input_check.__file__ = __file__
        sys.exit(input_check.launch())
