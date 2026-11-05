#!/usr/bin/env python3
"""Break a real client transport, preserve PTYs, then restart the isolated host."""
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
        child = subprocess.Popen(args, env=env, stdout=log, stderr=log)
        children.append(child)
        return child

    def pump(test, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            while GLib.MainContext.default().iteration(False): pass
            if test(): return
            time.sleep(.01)
        raise AssertionError("reconnect check timed out")

    def settle(seconds=.3):
        end = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= end)

    def key(*keys):
        run("xdotool", "key", "--clearmodifiers", *keys)
        settle()

    def copied():
        run("xdotool", "mousemove", "--window", window, "400", "580", "click", "1")
        clipboard.set("waiting")
        key("ctrl+shift+c")
        values = []
        clipboard.read_text_async(None, lambda c, t: values.append(c.read_text_finish(t)))
        pump(lambda: bool(values))
        return values[0] or ""

    def stop(child):
        child.terminate()
        child.wait(timeout=4)

    try:
        fixture = root / "fixture"
        fixture.write_text('''#!/usr/bin/python3
import os,select,tty
from pathlib import Path
tty.setraw(0)
root=Path(os.environ['TEST_ROOT'])
path=root/f'bytes-{os.getpid()}'
path.touch()
os.write(1,(''.join(f'line-{i}\\r\\n' for i in range(100))+f'ALIVE-{os.getpid()}\\r\\n中文 é').encode())
last=None
while True:
    if select.select([0],[],[],.01)[0]:
        data=os.read(0,4096)
        if not data:break
        with path.open('ab') as f:f.write(data)
        os.write(1,data)
    trigger=root/('output-'+str(os.getpid()))
    if trigger.exists() and trigger.stat().st_mtime_ns!=last:
        last=trigger.stat().st_mtime_ns
        os.write(1,trigger.read_bytes())
''')
        fixture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        proxy_binary = ROOT / "core/target/debug/pigeon-sidebar-proxy"
        real, proxy = root / "runtime/real.sock", root / "runtime/pigeon.sock"
        host = spawn("host", [str(server), "--socket", str(real), "--shell", str(fixture)])
        pump(real.exists)
        bridge = spawn("proxy", [str(proxy_binary), str(proxy), str(real), str(root / "wire1.log")])
        pump(proxy.exists)
        wrapper = root / "server"
        wrapper.write_text(f'#!/bin/sh\nprintf "%s\\n" "{proxy}"\n')
        wrapper.chmod(0o700)
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")], dict(os.environ, PIGEON_SERVER=str(wrapper), GTK_IM_MODULE="simple"))
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        pump(lambda: len(list(root.glob("bytes-*"))) == 1)
        first = next(root.glob("bytes-*"))
        pump(lambda: "ALIVE" in copied())
        key("a", "ctrl+shift+t")
        pump(lambda: len(list(root.glob("bytes-*"))) == 2)
        second = next(p for p in root.glob("bytes-*") if p != first)
        pump(lambda: f"ALIVE-{second.name[6:]}" in copied())
        key("b")
        settle(.3)
        before = copied()
        if os.environ.get("PIGEON_TEST_FULL_RECOVERY"):
            key("shift+Page_Up")
            settle(.4)
        stop(bridge); proxy.unlink(missing_ok=True)
        settle(.5)
        key("z")
        assert second.read_bytes() == b"b"
        assert host.poll() is None and app.poll() is None
        missed = bool(os.environ.get("PIGEON_TEST_MISSED_DELTA"))
        if missed:
            (root / ("output-" + second.name[6:])).write_text("\r\nWHILE-DISCONNECTED")
            settle(.3)
        bridge = spawn("proxy2", [str(proxy_binary), str(proxy), str(real), str(root / "wire2.log")],
                       dict(os.environ, PIGEON_TEST_RECOVERY_DELAY="1"))
        pump(lambda: (root / "wire2.log").exists())
        pump(lambda: "C attach 2" in (root / "wire2.log").read_text())
        if os.environ.get("PIGEON_TEST_FULL_RECOVERY"):
            key("y")
            assert second.read_bytes() == b"b", "handshake/layout cannot authorize stale-screen input"
        pump(lambda: "WHILE-DISCONNECTED" in copied() if missed else copied() == before)
        wire2 = (root / "wire2.log").read_text()
        if not os.environ.get("PIGEON_TEST_FULL_RECOVERY"):
            assert "C resume 2" in wire2, wire2
            assert "S snapshot 2" not in wire2, wire2
            if missed: assert "S delta 2" in wire2, wire2
        key("c")
        pump(lambda: second.read_bytes() == b"bc")
        assert first.read_bytes() == b"a"
        assert len(list(root.glob("bytes-*"))) == 2
        print("PASS same instance restores selected pane and exact Unicode screen without replaying offline input", flush=True)
        stop(bridge); proxy.unlink(missing_ok=True)
        stop(host); real.unlink(missing_ok=True)
        host = spawn("host2", [str(server), "--socket", str(real), "--shell", str(fixture)])
        pump(real.exists)
        bridge = spawn("proxy3", [str(proxy_binary), str(proxy), str(real), str(root / "wire3.log")])
        pump(lambda: (root / "wire3.log").exists())
        pump(lambda: "S sessions []" in (root / "wire3.log").read_text())
        settle(.6)
        key("x")
        assert len(list(root.glob("bytes-*"))) == 2
        assert second.read_bytes() == b"bc"
        assert "C attach" not in (root / "wire3.log").read_text()
        key("ctrl+shift+t")
        pump(lambda: len(list(root.glob("bytes-*"))) == 3)
        third = next(p for p in root.glob("bytes-*") if p not in (first, second))
        pump(lambda: f"ALIVE-{third.name[6:]}" in copied())
        key("d")
        pump(lambda: third.read_bytes() == b"d")
        print("PASS new instance clears stale identities; creating a new session remains explicit", flush=True)
        stop(bridge); proxy.unlink(missing_ok=True)
        settle(.3)
        run("xdotool", "windowclose", window)
        pump(lambda: app.poll() is not None)
        assert host.poll() is None
        print("PASS closing during recovery terminates client without stopping host", flush=True)
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
            for wire in root.glob("wire*.log"): print(wire.name, wire.read_text()[-4000:], file=sys.stderr)
        for log in logs: log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv: check()
    else:
        base.build_proxy()
        base.input_check.__file__ = __file__
        sys.exit(base.input_check.launch())
