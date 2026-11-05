#!/usr/bin/env python3
"""Isolated X11/GTK/fcitx5 input check; requires Xvfb, xdotool, PyGObject and fcitx5.

Run after building pigeon and pigeon-server: python3 tools/checks/linux-input-check.py
All displays, D-Bus services, clipboard contents and host processes belong to this test.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def run(*args):
    return subprocess.check_output(args, text=True, timeout=15).strip()


def launch():
    with tempfile.TemporaryDirectory(prefix="pigeon-input-") as directory:
        env = os.environ.copy()
        env.update(HOME=directory, XDG_CONFIG_HOME=f"{directory}/config",
                   XDG_DATA_HOME=f"{directory}/data", XDG_CACHE_HOME=f"{directory}/cache",
                   XDG_RUNTIME_DIR=f"{directory}/runtime", GDK_BACKEND="x11",
                   GTK_IM_MODULE="fcitx", PIGEON_FRAME_STATS="1", TEST_ROOT=directory,
                   GIO_USE_VFS="local", GTK_A11Y="none", NO_AT_BRIDGE="1")
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("DBUS_SESSION_BUS_ADDRESS", None)
        for name in ("config", "data", "cache", "runtime"):
            Path(directory, name).mkdir(mode=0o700)
        read_fd, write_fd = os.pipe()
        with open(Path(directory) / "xvfb.log", "w") as log:
            xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(write_fd), "-screen", "0",
                                     "1280x800x24", "-nolisten", "tcp"],
                                    pass_fds=(write_fd,), stdout=log, stderr=log)
            os.close(write_fd)
            try:
                with os.fdopen(read_fd) as display_pipe:
                    env["DISPLAY"] = ":" + display_pipe.readline().strip()
                return subprocess.call(["dbus-run-session", "--", sys.executable, __file__, "--inside"], env=env)
            finally:
                xvfb.terminate()
                xvfb.wait(timeout=3)


def check():
    temporary = Path(os.environ["TEST_ROOT"])
    children = []
    logs = []

    def spawn(name, args, **kwargs):
        log = open(temporary / f"{name}.log", "w+")
        logs.append(log)
        child = subprocess.Popen(args, stdout=log, stderr=log, **kwargs)
        children.append(child)
        return child

    try:
        import gi
        gi.require_version("Gtk", "4.0")
        from gi.repository import Gtk, Gdk, GLib
        Gtk.init()
        display = Gdk.Display.get_default()
        clipboard = display.get_clipboard()

        def pump_until(predicate, timeout=5):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                while GLib.MainContext.default().iteration(False):
                    pass
                if predicate():
                    return
                time.sleep(0.02)
            raise AssertionError("timed out waiting for input/clipboard")

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
        pump_until(lambda: subprocess.call(["fcitx5-remote", "--check"], stdout=subprocess.DEVNULL,
                                          stderr=subprocess.DEVNULL) == 0)
        capture = temporary / "capture.py"
        capture.write_text("""#!/usr/bin/python3
import os, tty
from pathlib import Path
tty.setraw(0)
path = Path(os.environ['TEST_ROOT']) / ('bytes-' + str(os.getpid()))
path.touch()
os.write(1, '\\x1b[2J\\x1b[HSCREEN-e\\u0301-中\\r\\n\\x1b[?2004h'.encode())
while True:
    data = os.read(0, 4096)
    if not data: break
    with path.open('ab') as output: output.write(data)
    if data == b'\\x02': os.write(1, b'\\x1b[5;10H')
    if data == b'\\x01': os.write(1, b'\\x1b[?2004l')
""")
        capture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        spawn("host", [str(server), "--socket", str(temporary / "runtime/pigeon.sock"), "--shell", str(capture)])
        pump_until(lambda: (temporary / "runtime/pigeon.sock").exists())
        os.environ["PIGEON_SERVER"] = str(server)
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")])
        pump_until(lambda: bool(list(temporary.glob("bytes-*"))))
        pane = next(temporary.glob("bytes-*"))
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        time.sleep(0.3)

        def key(*keys):
            run("xdotool", "key", "--clearmodifiers", *keys)
            until = time.monotonic() + 0.15
            pump_until(lambda: time.monotonic() >= until)

        def expect(data):
            pump_until(lambda: pane.read_bytes() == data)
            time.sleep(0.1)
            assert pane.read_bytes() == data, pane.read_bytes()

        key("ctrl+shift+b")
        run("xdotool", "windowsize", window, "640", "480")
        run("fcitx5-remote", "-c")
        # The sidebar starts the UI before its asynchronous first screen arrives.
        ready = []
        clipboard.set("waiting-for-first-screen")
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            key("ctrl+shift+c")
            clipboard.read_text_async(None, lambda clip, task: ready.append(clip.read_text_finish(task)))
            pump_until(lambda: bool(ready))
            text = ready.pop() or ""
            if text.lstrip("\n").startswith("SCREEN-"): break
        else: raise AssertionError("initial screen not ready")
        key("a", "b", "c", "ctrl+c", "alt+b", "Up")
        expected = b"abc\x03\x1bb\x1b[A"
        expect(expected)
        print("PASS ordinary keys / Ctrl / Alt / arrow exactly once", flush=True)

        run("fcitx5-remote", "-o")
        run("fcitx5-remote", "-s", "pinyin")
        key("n", "i", "h", "a", "o")
        assert pane.read_bytes() == expected, pane.read_bytes()
        screenshot = os.environ.get("PIGEON_INPUT_SCREENSHOT")
        if screenshot:
            run("import", "-window", "root", screenshot)
        key("space")
        expected += "你好".encode()
        expect(expected)
        print("PASS fcitx5 pinyin preedit sends no bytes; commit sends 你好 once", flush=True)
        run("fcitx5-remote", "-c")

        clipboard.set("paste-中文\nline\x1b[201~end")
        key("ctrl+shift+v")
        expected += "\x1b[200~paste-中文\rlineend\x1b[201~".encode()
        expect(expected)
        print("PASS asynchronous clipboard paste / host bracket markers / newline normalization", flush=True)
        clipboard.set("")
        key("ctrl+shift+v")
        expect(expected)
        key("ctrl+a")
        expected += b"\x01"
        clipboard.set("plain\r\ntext")
        key("ctrl+shift+v")
        expected += b"plain\rtext"
        expect(expected)
        print("PASS empty clipboard ignored / plain paste follows host mode", flush=True)

        key("ctrl+shift+c")
        result = []
        clipboard.read_text_async(None, lambda clip, task: result.append(clip.read_text_finish(task)))
        pump_until(lambda: bool(result))
        assert result[0].lstrip("\n").startswith("SCREEN-e\u0301-中\n"), repr(result[0])
        assert pane.read_bytes() == expected
        print("PASS copy current screen preserves combining text and CJK", flush=True)

        key("ctrl+shift+d")
        pump_until(lambda: len(list(temporary.glob("bytes-*"))) == 2)
        other = next(path for path in temporary.glob("bytes-*") if path != pane)
        key("x", "ctrl+shift+z", "ctrl+shift+z")
        pump_until(lambda: other.read_bytes() == b"x")
        assert pane.read_bytes() == expected
        print("PASS split / focused pane / zoom shortcuts do not reach programs", flush=True)
        key("ctrl+b")
        run("fcitx5-remote", "-o")
        key("n", "i", "h", "a", "o")
        assert other.read_bytes() == b"x\x02"
        if screenshot:
            run("import", "-window", "root", str(Path(screenshot).with_stem("pigeon-input-split-preedit")))
        # The clipboard owner deliberately does not run its main loop until focus has moved.
        clipboard.set("must-not-paste-into-a-different-pane")
        run("xdotool", "key", "--clearmodifiers", "ctrl+shift+v")
        time.sleep(0.15)
        run("xdotool", "mousemove", "--window", window, "60", "160", "click", "1")
        until = time.monotonic() + 0.3
        pump_until(lambda: time.monotonic() >= until)
        run("fcitx5-remote", "-c")
        key("q")
        expected += b"q"
        expect(expected)
        assert other.read_bytes() == b"x\x02", other.read_bytes()
        print("PASS pane switch cancels preedit and pending async paste", flush=True)
        geometry = dict(line.split("=", 1) for line in run("xdotool", "getwindowgeometry", "--shell", window).splitlines())
        run("xdotool", "mousemove", "--window", window, str(int(geometry["WIDTH"]) - 40), "160", "click", "1")
        key("ctrl+shift+w")
        key("y")
        expected += b"y"
        expect(expected)
        print("PASS close restores live pane focus", flush=True)
        run("fcitx5-remote", "-o")
        key("n", "i")
        focus_window = Gtk.Window(title="Isolated focus target")
        focus_window.present()
        until = time.monotonic() + 0.2
        pump_until(lambda: time.monotonic() >= until)
        focus_id = run("xdotool", "search", "--onlyvisible", "--name", "Isolated focus target").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", focus_id)
        until = time.monotonic() + 0.2
        pump_until(lambda: time.monotonic() >= until)
        run("xdotool", "windowfocus", "--sync", window)
        run("fcitx5-remote", "-c")
        key("r")
        expected += b"r"
        expect(expected)
        focus_window.destroy()
        print("PASS window blur cancels preedit and refocus restores keys", flush=True)
        # A separate session uses the same IM context but must not inherit a pending commit/paste.
        key("ctrl+shift+t")
        pump_until(lambda: len(list(temporary.glob("bytes-*"))) == 3)
        third = next(path for path in temporary.glob("bytes-*") if path not in (pane, other))
        until = time.monotonic() + 0.6
        pump_until(lambda: time.monotonic() >= until)
        run("fcitx5-remote", "-c")
        key("s")
        pump_until(lambda: third.read_bytes() == b"s")
        run("fcitx5-remote", "-o")
        key("n", "i")
        clipboard.set("must-not-cross-session")
        run("xdotool", "key", "--clearmodifiers", "ctrl+shift+v")
        time.sleep(0.1)
        run("xdotool", "key", "--clearmodifiers", "ctrl+shift+bracketleft")
        until = time.monotonic() + 0.7
        pump_until(lambda: time.monotonic() >= until)
        run("fcitx5-remote", "-c")
        key("u")
        expected += b"u"
        expect(expected)
        assert third.read_bytes() == b"s", third.read_bytes()
        print("PASS session switch cancels preedit and pending asynchronous paste", flush=True)
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
        if sys.exc_info()[0]:
            for log in logs:
                log.flush()
                log.seek(0)
                print(f"--- {log.name} ---\n{log.read()[-5000:]}", file=sys.stderr)
            for path in temporary.glob("bytes-*"):
                print(path.name, repr(path.read_bytes()), file=sys.stderr)
        for log in logs:
            log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv:
        check()
    else:
        for tool in ("Xvfb", "xdotool", "fcitx5", "fcitx5-remote", "dbus-run-session"):
            if not shutil.which(tool):
                sys.exit(f"Missing test dependency: {tool}")
        sys.exit(launch())
