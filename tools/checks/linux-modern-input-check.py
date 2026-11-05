#!/usr/bin/env python3
"""Isolated GTK -> real PTY protocol and GIO mock opener acceptance (no real browser).

Uses the existing Xvfb/HOME/D-Bus lifecycle. Build pigeon and pigeon-server first.
"""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("input_check", Path(__file__).with_name("linux-input-check.py"))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
run = base.run


def check():
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, GLib, Gdk
    Gtk.init()
    clipboard = Gdk.Display.get_default().get_clipboard()
    root = Path(os.environ["TEST_ROOT"])
    children, logs = [], []

    def spawn(name, args):
        log = open(root / f"{name}.log", "w+")
        logs.append(log)
        child = subprocess.Popen(args, stdout=log, stderr=log, cwd=root)
        children.append(child)
        return child

    def pump(test, timeout=8):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            while GLib.MainContext.default().iteration(False):
                pass
            if test(): return
            time.sleep(.01)
        raise AssertionError("timed out waiting for GTK/PTY evidence")

    def settle(seconds=.2):
        deadline = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= deadline)

    def key(*keys):
        run("xdotool", "key", "--clearmodifiers", *keys)
        settle()

    def command(text):
        (root / "output").write_text(text)
        settle(.3)

    def capture(action):
        before = pane.read_bytes()
        action()
        settle()
        return pane.read_bytes()[len(before):]

    def expect(action, expected):
        actual = capture(action)
        assert actual == expected, (actual, expected)

    try:
        config = root / "config/fcitx5"
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
        pump(lambda: subprocess.call(["fcitx5-remote", "--check"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) == 0)
        run("fcitx5-remote", "-c")
        fixture = root / "capture.py"
        fixture.write_text('''#!/usr/bin/python3
import os, select, tty
from pathlib import Path
tty.setraw(0)
root = Path(os.environ['TEST_ROOT'])
path = root / ('bytes-' + str(os.getpid()))
path.touch()
os.write(1, b'READY')
last = None
while True:
    if select.select([0], [], [], .01)[0]:
        data = os.read(0, 4096)
        if not data: break
        with path.open('ab') as output: output.write(data)
    trigger = root / 'output'
    if trigger.exists() and (stamp := trigger.stat().st_mtime_ns) != last:
        last = stamp
        os.write(1, trigger.read_bytes())
''')
        fixture.chmod(0o700)
        # A sandbox-only registered HTTP handler exercises the actual GIO call, without a browser.
        opener = root / "opener.py"
        opener.write_text('''#!/usr/bin/python3
import os, sys
from pathlib import Path
with (Path(os.environ['TEST_ROOT']) / 'opened').open('a') as out:
    out.write(repr(sys.argv[1:]) + '\\n')
''')
        opener.chmod(0o700)
        apps = root / "data/applications"
        apps.mkdir()
        (apps / "pigeon-test.desktop").write_text(f"[Desktop Entry]\nType=Application\nName=Pigeon test opener\nExec={opener} %u\nMimeType=x-scheme-handler/http;x-scheme-handler/https;\n")
        (root / "config/mimeapps.list").write_text("[Default Applications]\nx-scheme-handler/http=pigeon-test.desktop;\nx-scheme-handler/https=pigeon-test.desktop;\n")
        server = ROOT / "core/target/debug/pigeon-server"
        spawn("host", [str(server), "--socket", str(root / "runtime/pigeon.sock"), "--shell", str(fixture)])
        pump(lambda: (root / "runtime/pigeon.sock").exists())
        os.environ["PIGEON_SERVER"] = str(server)
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")])
        pump(lambda: bool(list(root.glob("bytes-*"))))
        pane = next(root.glob("bytes-*"))
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        settle(.8)
        key("ctrl+shift+b")
        run("xdotool", "windowsize", window, "640", "480")
        settle(.4)
        run("xdotool", "windowfocus", "--sync", window)
        run("xdotool", "mousemove", "--window", window, "320", "200", "click", "1")
        clipboard.set("waiting")
        ready = []
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            key("ctrl+shift+c")
            clipboard.read_text_async(None, lambda c,t: ready.append(c.read_text_finish(t)))
            pump(lambda: bool(ready))
            if "READY" in (ready.pop() or ""): break
        else: raise AssertionError("initial screen not ready")
        run("fcitx5-remote", "-c")
        expect(lambda: key("a"), b"a")
        command("\x1b[?1h\x1b=")
        expect(lambda: key("Up", "KP_Enter"), b"\x1bOA\x1bOM")
        command("\x1b[?1l\x1b>")
        print("PASS legacy DECCKM / DECKPAM", flush=True)

        before = pane.read_bytes()
        command("\x1b[>3u\x1b[?u")
        assert pane.read_bytes()[len(before):] == b"\x1b[?3u"
        expect(lambda: key("ctrl+i", "Tab", "shift+Return"), b"\x1b[105;5u\x1b[105;1:3u\t\x1b[13;2u")
        command("\x1b[<u\x1b[>31u")
        expect(lambda: key("a"), b"\x1b[97;;97u\x1b[97;1:3u")
        def repeat():
            run("xdotool", "keydown", "a")
            settle(.8)
            run("xdotool", "keyup", "a")
        repeated = capture(repeat)
        assert repeated.startswith(b"\x1b[97;;97u") and b"\x1b[97;1:2;97u" in repeated and repeated.endswith(b"\x1b[97;1:3u"), repeated
        modifier = capture(lambda: key("Shift_L"))
        assert modifier == b"\x1b[57441;2u\x1b[57441;1:3u", modifier
        print("PASS Kitty query / disambiguation / press-repeat-release / modifier", flush=True)

        command("\x1b[<u\x1b[>3u")
        def chord_repeat():
            run("xdotool", "keydown", "ctrl", "keydown", "a", "keyup", "ctrl")
            settle(.8)
            run("xdotool", "keyup", "a")
        repeated = capture(chord_repeat)
        assert repeated.endswith(b"\x1b[97;1:3u"), repeated
        command("\x1b[<u\x1b[?1h\x1b[>1u")
        def cursor_repeat():
            run("xdotool", "keydown", "Up")
            settle(.8)
            run("xdotool", "keyup", "Up")
        repeated = capture(cursor_repeat)
        assert repeated.count(b"\x1bOA") > 1 and b"\x1b[A" not in repeated, repeated
        command("\x1b[<u\x1b[?1l\x1b[>31u")
        print("PASS repeats retain original release and DECCKM without event types", flush=True)

        run("fcitx5-remote", "-o")
        run("fcitx5-remote", "-s", "pinyin")
        expect(lambda: key("n", "i", "h", "a", "o"), b"")
        expect(lambda: key("space"), b"\x1b[0;;20320:22909u")
        run("fcitx5-remote", "-c")
        command("\x1b[<u")
        print("PASS Kitty + fcitx5 preedit zero bytes / one text commit / no orphan release", flush=True)

        command("\x1b[?1000h\x1b[?1006h")
        run("xdotool", "mousemove", "--window", window, "20", "20")
        expect(lambda: run("xdotool", "click", "1"), b"\x1b[<0;3;2M\x1b[<0;3;2m")
        expect(lambda: run("xdotool", "click", "4"), b"\x1b[<64;3;2M" * 3)
        command("\x1b[?1002h")
        def drag():
            run("xdotool", "mousedown", "1", "mousemove", "--window", window, "28", "20", "mouseup", "1")
        expect(drag, b"\x1b[<0;3;2M\x1b[<32;4;2M\x1b[<0;4;2m")
        command("\x1b[?1003h")
        expect(lambda: run("xdotool", "mousemove", "--window", window, "36", "20"), b"\x1b[<35;5;2M")
        command("\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1004h")
        focus_start = pane.read_bytes()
        target = Gtk.Window(title="Modern input focus target")
        target.present()
        settle()
        focus_id = run("xdotool", "search", "--onlyvisible", "--name", "Modern input focus target").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", focus_id)
        settle()
        assert pane.read_bytes()[len(focus_start):] == b"\x1b[O"
        expect(lambda: run("xdotool", "windowfocus", "--sync", window), b"\x1b[I")
        target.destroy()
        command("\x1b[?1004l")
        print("PASS raw PTY mouse click/release/wheel/drag/motion and focus reports", flush=True)

        command("".join(f"HISTORY-{i}\r\n" for i in range(100)) + "\x1b[?1000h")
        key("shift+Prior")
        expect(lambda: run("xdotool", "click", "1", "click", "4"), b"")
        key("shift+End")
        command("\x1b[?1000l\x1b[>11u")
        key("shift+Prior")
        copied = []
        key("ctrl+shift+c")
        clipboard.read_text_async(None, lambda c, t: copied.append(c.read_text_finish(t)))
        pump(lambda: bool(copied))
        assert "HISTORY-99" not in copied[0], copied
        expect(lambda: key("a"), b"\x1b[97u\x1b[97;1:3u")
        command("\x1b[<u")
        command("\x1b[?1004h")
        expect(lambda: key("shift+Prior"), b"\x1b[O")
        expect(lambda: key("shift+End"), b"\x1b[I")
        command("\x1b[?1004l")
        print("PASS history clicks/wheel stay local; Kitty typing returns live with paired release", flush=True)

        def link(uri):
            command(f"\x1b[2J\x1b[H\x1b]8;;{uri}\x1b\\LINK\x1b]8;;\x1b\\")
        def click():
            run("xdotool", "keydown", "ctrl", "click", "1", "keyup", "ctrl")
            settle()
        def opened():
            path = root / "opened"
            return path.read_text().splitlines() if path.exists() else []
        link("https://example.com/test")
        run("xdotool", "mousemove", "--window", window, "12", "8")
        click()
        pump(lambda: len(opened()) == 1)
        assert opened() == ["['https://example.com/test']"], opened()
        command("\x1b[?1000h\x1b[?1006h")
        expect(click, b"")
        pump(lambda: len(opened()) == 2)
        command("\x1b[?1000l")
        run("xdotool", "keydown", "ctrl", "mousedown", "1", "click", "4", "mouseup", "1", "keyup", "ctrl")
        settle()
        assert len(opened()) == 2, opened()
        run("xdotool", "click", "1")
        run("xdotool", "keydown", "ctrl", "mousedown", "1", "mousemove", "--window", window, "13", "8", "mouseup", "1", "keyup", "ctrl")
        run("xdotool", "keydown", "ctrl", "mousedown", "1")
        link("https://example.org/changed")
        link("https://example.com/test")
        run("xdotool", "mouseup", "1", "keyup", "ctrl")
        run("xdotool", "keydown", "ctrl", "mousedown", "1")
        command("\x1b[2;1Hupdate")
        run("xdotool", "mouseup", "1", "keyup", "ctrl")
        for uri in ("javascript:alert(1)", "https://user:pass@example.com", "https://example.com/%00", "https://example.com:65536"):
            link(uri)
            click()
        settle()
        assert len(opened()) == 2, opened()
        print("PASS GIO mock opener: explicit complete Ctrl-click only; drag/update/unsafe URI cancel", flush=True)
        assert app.poll() is None
        client_log = (root / "client.log").read_text()
        assert "panicked" not in client_log and "CRITICAL" not in client_log, client_log
        print("PASS no GTK critical or Rust panic", flush=True)
    finally:
        for child in reversed(children):
            child.terminate()
            try: child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        if sys.exc_info()[0]:
            for log in logs:
                log.flush()
                log.seek(0)
                print(f"--- {log.name} ---\n{log.read()[-3000:]}", file=sys.stderr)
            for path in root.glob("bytes-*"):
                print(path.name, repr(path.read_bytes()), file=sys.stderr)
        for log in logs: log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv:
        check()
    else:
        base.__file__ = __file__
        sys.exit(base.launch())
