#!/usr/bin/env python3
"""Isolated real GTK search, generation-bound viewport and cancelled replies."""
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
    from gi.repository import Gtk, Gdk, GLib
    Gtk.init()
    clipboard = Gdk.Display.get_default().get_clipboard()
    root = Path(os.environ["TEST_ROOT"])
    children, logs = [], []

    def spawn(name, args, env=None):
        log = (root / (name + ".log")).open("w+")
        logs.append(log)
        process = subprocess.Popen(args, env=env, stdout=log, stderr=log)
        children.append(process)
        return process

    def pump(test, timeout=12):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            while GLib.MainContext.default().iteration(False): pass
            if test(): return
            time.sleep(.01)
        raise AssertionError("search check timed out")

    def settle(seconds=.25):
        end = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= end)

    def key(*keys):
        run("xdotool", "key", "--clearmodifiers", *keys)
        settle()

    def lines():
        return wire.read_text().splitlines() if wire.exists() else []

    def search(text):
        key("ctrl+shift+f", "ctrl+a")
        run("xdotool", "type", "--clearmodifiers", text)
        settle(.8)

    def copy():
        run("xdotool", "mousemove", "--window", window, "400", "580", "click", "1")
        key("ctrl+shift+c")
        result = []
        clipboard.read_text_async(None, lambda c, t: result.append(c.read_text_finish(t)))
        pump(lambda: bool(result))
        return result[0]

    try:
        fixture = root / "fixture"
        fixture.write_text('''#!/usr/bin/python3
import os,select,tty
from pathlib import Path
tty.setraw(0)
root=Path(os.environ['TEST_ROOT'])
(root/'bytes').touch()
text=''.join(f'row {i:04d} needle 中文 é\\r\\n' for i in range(300))+'LIVE-END'
os.write(1,text.encode())
last=None
while True:
    if select.select([0],[],[],.01)[0]:
        data=os.read(0,4096)
        if not data: break
        with (root/'bytes').open('ab') as f:f.write(data)
    trigger=root/'output'
    if trigger.exists() and trigger.stat().st_mtime_ns!=last:
        last=trigger.stat().st_mtime_ns
        os.write(1,trigger.read_bytes())
''')
        fixture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        real, proxy, wire = root / "runtime/real.sock", root / "runtime/pigeon.sock", root / "wire.log"
        spawn("host", [str(server), "--socket", str(real), "--shell", str(fixture)])
        pump(real.exists)
        env = dict(os.environ, PIGEON_TEST_SEARCH_DELAY="1")
        spawn("proxy", [str(ROOT / "core/target/debug/pigeon-sidebar-proxy"), str(proxy), str(real), str(wire)], env)
        pump(proxy.exists)
        wrapper = root / "server"
        wrapper.write_text(f'#!/bin/sh\nprintf "%s\\n" "{proxy}"\n')
        wrapper.chmod(0o700)
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")], dict(os.environ, PIGEON_SERVER=str(wrapper), GTK_IM_MODULE="simple"))
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        run("xdotool", "windowsize", window, "1100", "740")
        pump(lambda: any(l.startswith("C resize") for l in lines()))
        settle(.8)
        if os.environ.get("PIGEON_TEST_NO_SEARCH"):
            key("ctrl+shift+f")
            assert not any(l.startswith("C query") for l in lines())
            print("PASS missing capability sends no query")
            return
        search("needle")
        pump(lambda: any("Viewport" in l for l in lines()))
        settle(1)
        text = copy()
        assert "row 0000 needle 中文 é" in text and "LIVE-END" not in text, text
        assert (root / "bytes").read_bytes() == b""
        print("PASS real history search and same-generation picture preserve Unicode", flush=True)
        if os.environ.get("PIGEON_SEARCH_SCREENSHOT"):
            run("import", "-window", window, os.environ["PIGEON_SEARCH_SCREENSHOT"])
        search("needle")
        key("Return")
        settle(.6)
        assert sum("Viewport" in l for l in lines()) >= 2
        key("Escape")
        settle(.5)
        assert "LIVE-END" in copy()
        key("x")
        pump(lambda: (root / "bytes").read_bytes() == b"x")
        print("PASS next match / Escape / live input restore", flush=True)
        search("needle")
        key("Escape")
        settle(.6)
        assert "LIVE-END" in copy()
        print("PASS delayed results cannot replace the closed search", flush=True)
        search("needle")
        (root / "output").write_text("\r\nNEW-OUTPUT")
        settle(.6)
        key("Escape")
        settle(.6)
        assert "NEW-OUTPUT" in copy()
        print("PASS output invalidates search and live recovery gets current content", flush=True)
        search("needle")
        key("ctrl+shift+t")
        settle(.8)
        assert "panicked" not in (root / "client.log").read_text()
        key("ctrl+shift+bracketleft")
        settle(.6)
        assert "NEW-OUTPUT" in copy()
        print("PASS session switching cancels lookup and restores the previous live pane", flush=True)
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
                log.flush(); log.seek(0); print(log.name, log.read()[-4000:], file=sys.stderr)
            if 'wire' in locals() and wire.exists(): print(wire.read_text()[-7000:], file=sys.stderr)
        for log in logs: log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv: check()
    else:
        base.build_proxy()
        base.input_check.__file__ = __file__
        sys.exit(base.input_check.launch())
