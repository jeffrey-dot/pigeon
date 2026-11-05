#!/usr/bin/env python3
"""Visible-grid selection over real GTK/PTY, including application mouse override."""
import importlib.util
import os
from pathlib import Path
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
    from gi.repository import Gtk, Gdk, GLib
    Gtk.init()
    clipboard = Gdk.Display.get_default().get_clipboard()
    root = Path(os.environ["TEST_ROOT"])
    children, logs = [], []

    def spawn(name, args):
        log = (root / (name + ".log")).open("w+")
        logs.append(log)
        child = subprocess.Popen(args, stdout=log, stderr=log)
        children.append(child)
        return child

    def pump(test, timeout=8):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            while GLib.MainContext.default().iteration(False): pass
            if test(): return
            time.sleep(.01)
        raise AssertionError("selection check timed out")

    def settle(seconds=.25):
        end = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= end)

    def key(*keys):
        run("xdotool", "key", "--clearmodifiers", *keys)
        settle()

    def command(text):
        (root / "output").write_text(text)
        settle(.4)

    def copy():
        clipboard.set("unchanged")
        key("ctrl+shift+c")
        values = []
        clipboard.read_text_async(None, lambda c, t: values.append(c.read_text_finish(t)))
        pump(lambda: bool(values))
        return values[0]

    def point(col, row):
        return (309 + col * 8, 110 + row * 15)

    def move(col, row):
        run("xdotool", "mousemove", "--window", window, *map(str, point(col, row)))

    def drag(a, b, modifier=None):
        if modifier: run("xdotool", "keydown", modifier)
        move(*a); run("xdotool", "mousedown", "1"); settle(.1)
        move(*b); settle(.1); run("xdotool", "mouseup", "1")
        if modifier: run("xdotool", "keyup", modifier)
        settle()

    try:
        fixture = root / "fixture"
        fixture.write_text('''#!/usr/bin/python3
import os,select,tty
from pathlib import Path
tty.setraw(0)
root=Path(os.environ['TEST_ROOT']);(root/'bytes').touch()
os.write(1,'abc中é xyz\\r\\nsrc/main.rs:12 rest\\r\\nthird row'.encode())
last=None
while True:
    if select.select([0],[],[],.01)[0]:
        data=os.read(0,4096)
        if not data:break
        with (root/'bytes').open('ab') as f:f.write(data)
    path=root/'output'
    if path.exists() and path.stat().st_mtime_ns!=last:
        last=path.stat().st_mtime_ns;os.write(1,path.read_bytes())
''')
        fixture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        socket = root / "runtime/pigeon.sock"
        spawn("host", [str(server), "--socket", str(socket), "--shell", str(fixture)])
        pump(socket.exists)
        os.environ.update(PIGEON_SERVER=str(server), PIGEON_FONT="DejaVu Sans Mono", GTK_IM_MODULE="simple")
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")])
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowsize", window, "1100", "740")
        run("xdotool", "windowfocus", "--sync", window)
        settle(1)
        pump(lambda: "abc中é" in copy())
        drag((4,0),(5,0))
        assert copy() == "中é"
        drag((5,0),(4,0))
        assert copy() == "中é"
        settle(.5)
        move(5,1); run("xdotool", "click", "--repeat", "2", "--delay", "80", "1"); settle()
        assert copy() == "src/main.rs:12"
        settle(.5)
        move(3,2); run("xdotool", "click", "--repeat", "3", "--delay", "80", "1"); settle()
        assert copy() == "third row"
        drag((0,0),(2,1),"alt")
        assert copy() == "abc\nsrc"
        print("PASS drag/reverse drag, wide+combining cells, word/line and rectangle copy", flush=True)
        key("ctrl+shift+a")
        assert "abc中é xyz\nsrc/main.rs:12 rest\nthird row" in copy()
        command("\x1b[2J\x1b[Hupdated")
        assert copy().startswith("updated")
        print("PASS visible Select All and output invalidates obsolete selection", flush=True)
        command("\x1b[2J\x1b[Habc中é xyz\x1b[?1000h\x1b[?1006h")
        before = (root / "bytes").read_bytes()
        drag((0,0),(2,0),"shift")
        assert copy() == "abc"
        assert (root / "bytes").read_bytes() == before
        command("\x1b[?1000l")
        key("x")
        pump(lambda: (root / "bytes").read_bytes().endswith(b"x"))
        print("PASS Shift selection overrides application mouse; typing clears selection", flush=True)
        assert app.poll() is None
        log = (root / "client.log").read_text()
        assert "panicked" not in log and "CRITICAL" not in log, log
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try: child.wait(timeout=3)
                except subprocess.TimeoutExpired: child.kill(); child.wait()
        if sys.exc_info()[0]:
            for log in logs:
                log.flush();log.seek(0);print(log.name,log.read()[-3000:],file=sys.stderr)
        for log in logs:log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv: check()
    else:
        base.__file__ = __file__
        sys.exit(base.launch())
