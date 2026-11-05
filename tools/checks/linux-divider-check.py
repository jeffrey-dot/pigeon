#!/usr/bin/env python3
"""Real GTK divider drags sent through the production protocol to an isolated PTY host."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("sidebar", Path(__file__).with_name("linux-sidebar-check.py"))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
run = base.run


def check():
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, GLib
    Gtk.init()
    root = Path(os.environ["TEST_ROOT"])
    children, logs = [], []
    wire = root / "wire.log"

    def spawn(name, args):
        log = (root / (name + ".log")).open("w+")
        logs.append(log)
        child = subprocess.Popen(args, stdout=log, stderr=log)
        children.append(child)
        return child

    def pump(test, timeout=10):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            while GLib.MainContext.default().iteration(False): pass
            if test(): return
            time.sleep(.01)
        raise AssertionError("divider check timed out")

    def settle(seconds=.3):
        end = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= end)

    def lines():
        return wire.read_text().splitlines() if wire.exists() else []

    def key(*keys):
        run("xdotool", "key", "--clearmodifiers", *keys)
        settle()

    def drag(start, finish):
        run("xdotool", "mousemove", "--window", window, *map(str, start), "mousedown", "1")
        settle(.1)
        run("xdotool", "mousemove", "--sync", "--window", window, *map(str, finish))
        settle(.12)
        run("xdotool", "mouseup", "1")
        settle(.4)

    def ratios(split):
        return [float(line.split()[-1]) for line in lines() if line.startswith(f"C resize_split {split} ")]

    try:
        fixture = root / "fixture"
        fixture.write_text('''#!/usr/bin/python3
import os,tty
from pathlib import Path
tty.setraw(0)
path=Path(os.environ['TEST_ROOT'])/f'bytes-{os.getpid()}'
path.touch()
os.write(1,b'READY\\x1b[?1000h\\x1b[?1006h')
while True:
    data=os.read(0,4096)
    if not data:break
    with path.open('ab') as f:f.write(data)
''')
        fixture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        real, proxy = root / "runtime/real.sock", root / "runtime/pigeon.sock"
        spawn("host", [str(server), "--socket", str(real), "--shell", str(fixture)])
        pump(real.exists)
        spawn("proxy", [str(ROOT / "core/target/debug/pigeon-sidebar-proxy"), str(proxy), str(real), str(wire)])
        pump(proxy.exists)
        wrapper = root / "server"
        wrapper.write_text(f'#!/bin/sh\nprintf "%s\\n" "{proxy}"\n'); wrapper.chmod(0o700)
        os.environ.update(PIGEON_SERVER=str(wrapper), GTK_IM_MODULE="simple")
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")])
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowsize", window, "1100", "740")
        run("xdotool", "windowfocus", "--sync", window)
        pump(lambda: any(l.startswith("C resize 1 ") for l in lines())); settle(.5)
        key("ctrl+shift+d")
        pump(lambda: len(list(root.glob("bytes-*"))) == 2); settle(.5)
        drag((686,300),(830,300))
        pump(lambda: bool(ratios(1)) and .65 < ratios(1)[-1] < .75)
        assert not any(path.read_bytes() for path in root.glob("bytes-*")), "divider gesture leaked mouse bytes"
        print("PASS vertical ratio changes through host without application mouse input", flush=True)
        key("ctrl+shift+e")
        pump(lambda: len(list(root.glob("bytes-*"))) == 3); settle(.5)
        drag((950,405),(950,525))
        pump(lambda: bool(ratios(2)) and .6 < ratios(2)[-1] < .8)
        assert not any(path.read_bytes() for path in root.glob("bytes-*"))
        print("PASS nested horizontal divider uses its own host-provided box", flush=True)
        key("ctrl+shift+z")
        count = len(ratios(1)) + len(ratios(2))
        drag((830,300),(860,300))
        assert len(ratios(1)) + len(ratios(2)) == count
        key("ctrl+shift+z")
        print("PASS zoom hides divider hit targets", flush=True)
        # Drag beyond the canvas; the core clamps pane minima and the final release stays consumed.
        drag((830,250),(1055,250))
        pump(lambda: ratios(1)[-1] > .9)
        assert app.poll() is None
        assert "panicked" not in (root / "client.log").read_text()
        print("PASS core-clamped extreme ratio keeps the client alive", flush=True)
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try: child.wait(timeout=3)
                except subprocess.TimeoutExpired: child.kill(); child.wait()
        if sys.exc_info()[0]:
            for log in logs:
                log.flush(); log.seek(0); print(log.name, log.read()[-3000:], file=sys.stderr)
            if wire.exists(): print(wire.read_text()[-7000:], file=sys.stderr)
        for log in logs: log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv: check()
    else:
        base.build_proxy()
        base.input_check.__file__ = __file__
        sys.exit(base.input_check.launch())
