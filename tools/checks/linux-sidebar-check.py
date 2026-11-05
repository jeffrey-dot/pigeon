#!/usr/bin/env python3
"""Isolated real GTK multi-session/status acceptance; uses production Rust wire APIs."""
import importlib.util
import os
from pathlib import Path
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
    with tempfile.TemporaryDirectory(prefix="pigeon-sidebar-build-") as directory:
        manifest = Path(directory) / "Cargo.toml"
        manifest.write_text(f'''[package]
name = "pigeon-sidebar-proxy"
version = "0.0.0"
edition = "2021"
license = "GPL-3.0-or-later"
[dependencies]
pigeon-core = {{ path = "{ROOT / 'core'}", default-features = false }}
[[bin]]
name = "pigeon-sidebar-proxy"
path = "{ROOT / 'tools/checks/helpers/linux-sidebar-proxy.rs'}"
''')
        subprocess.run(["cargo", "build", "--offline", "--quiet", "--manifest-path", str(manifest),
                        "--target-dir", str(ROOT / "core/target")], check=True)


def check():
    temporary = Path(os.environ["TEST_ROOT"])
    children, logs = [], []
    wire = temporary / "wire.log"

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
            time.sleep(0.02)
        raise AssertionError("timed out waiting for sidebar evidence")

    def settle(seconds=0.3):
        end = time.monotonic() + seconds
        pump(lambda: time.monotonic() >= end)

    def key(*keys):
        for value in keys:
            run("xdotool", "key", "--clearmodifiers", value)
            settle(0.5)

    def messages():
        return wire.read_text().splitlines() if wire.exists() else []

    def walk(node):
        yield node
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child:
                yield from walk(child)

    def nodes():
        return list(walk(Atspi.get_desktop(0)))

    def find(name, role=None):
        return next((n for n in nodes() if n.get_name() == name and (role is None or n.get_role_name() == role)), None)

    def click(name):
        pump(lambda: find(name, "button") is not None)
        node = find(name, "button")
        node.get_action_iface().do_action(0)
        settle()

    def select_record(identifier):
        index = {"blocked-a": 1, "blocked-b": 2, "failed": 3, "finished": 4}[identifier]
        key("Home", *(["Down"] * index))
        settle()

    def command(pane, text):
        (temporary / f"output-{pane}").write_text(text)
        settle()

    def report(pane, identity, state):
        import base64
        message = base64.b64encode(("Full plain message " + identity + " <b>literal</b>").encode()).decode()
        command(pane, f"\x1b]7501;id={identity}:state={state}:app=test:msg={message}\x07")

    try:
        # Own the accessibility bus too: never activate the user's systemd accessibility unit.
        config = temporary / "a11y.conf"
        config.write_text('<busconfig><type>accessibility</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        bus = subprocess.Popen(["dbus-daemon", "--nofork", "--print-address=1", "--config-file=" + str(config)], stdout=subprocess.PIPE, text=True)
        children.append(bus)
        os.environ["AT_SPI_BUS_ADDRESS"] = bus.stdout.readline().strip()
        spawn("registry", ["/usr/lib/at-spi2-registryd"])
        time.sleep(0.2)
        import gi
        gi.require_version("Gtk", "4.0")
        gi.require_version("Atspi", "2.0")
        from gi.repository import Gtk, Gdk, GLib, Atspi
        Gtk.init()
        clipboard = Gdk.Display.get_default().get_clipboard()
        capture = temporary / "capture.py"
        capture.write_text('''#!/usr/bin/python3
import os, select, tty
from pathlib import Path
tty.setraw(0)
root = Path(os.environ['TEST_ROOT'])
# The server allocates sequential panes; the test creates each only after the previous starts.
number = len(list(root.glob('bytes-*'))) + 1
path = root / f'bytes-{number}'
path.touch()
os.write(1, (''.join(f'PANE-{number}-LINE-{i:05d}\\r\\n' for i in range(120)) + f'PANE-{number}').encode())
last = None
while True:
    if select.select([0], [], [], 0.01)[0]:
        data = os.read(0, 4096)
        if not data: break
        with path.open('ab') as out: out.write(data)
    trigger = root / f'output-{number}'
    if trigger.exists() and (stamp := trigger.stat().st_mtime_ns) != last:
        last = stamp
        os.write(1, trigger.read_bytes())
''')
        capture.chmod(0o700)
        server = ROOT / "core/target/debug/pigeon-server"
        real = temporary / "runtime/real.sock"
        proxy = temporary / "runtime/pigeon.sock"
        host = spawn("host", [str(server), "--socket", str(real), "--shell", str(capture)])
        pump(real.exists)
        spawn("proxy", [str(ROOT / "core/target/debug/pigeon-sidebar-proxy"), str(proxy), str(real), str(wire)])
        pump(proxy.exists)
        wrapper = temporary / "server"
        wrapper.write_text(f'#!/bin/sh\nprintf "%s\\n" "{proxy}"\n')
        wrapper.chmod(0o700)
        env = os.environ.copy()
        env.update(PIGEON_SERVER=str(wrapper), GTK_A11Y="atspi", NO_AT_BRIDGE="0", GTK_IM_MODULE="simple")
        app = spawn("client", [str(ROOT / "core/target/debug/pigeon")], env=env)
        pump(lambda: (temporary / "bytes-1").exists())
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        run("xdotool", "windowsize", window, "1100", "700")
        pump(lambda: any(m.startswith("C resize 1 ") for m in messages()))
        settle(0.5)
        key("a")
        pump(lambda: (temporary / "bytes-1").read_bytes() == b"a")
        key("ctrl+shift+t")
        pump(lambda: (temporary / "bytes-2").exists())
        key("b")
        pump(lambda: (temporary / "bytes-2").read_bytes() == b"b")
        key("ctrl+shift+bracketleft", "c")
        pump(lambda: (temporary / "bytes-1").read_bytes() == b"ac")
        key("ctrl+shift+bracketright", "d")
        pump(lambda: (temporary / "bytes-2").read_bytes() == b"bd")
        print("PASS create and repeated session switches preserve input targets", flush=True)
        key("ctrl+shift+d")
        pump(lambda: (temporary / "bytes-3").exists())
        key("e", "ctrl+shift+z", "ctrl+shift+z")
        pump(lambda: (temporary / "bytes-3").read_bytes() == b"e")
        key("ctrl+shift+bracketleft", "f", "ctrl+shift+bracketright", "g")
        pump(lambda: (temporary / "bytes-3").read_bytes() == b"eg")
        assert (temporary / "bytes-1").read_bytes() == b"acf"
        print("PASS session-level layout subscription restores split focus and zoom", flush=True)
        # Create an unvisited third session via a separate safe-API connection.
        other_project = temporary / "another-project"
        other_project.mkdir()
        subprocess.run([str(ROOT / "core/target/debug/pigeon-sidebar-proxy"), "--seed", str(real), str(other_project)], check=True)
        pump(lambda: (temporary / "bytes-4").exists())
        report(4, "unvisited", "blocked")
        pump(lambda: any(m.startswith("S status 4 ") and "unvisited" in m for m in messages()))
        assert not any(m == "C attach 4" or m.startswith("C resize 4 ") for m in messages())
        assert any("another-project" in n.get_name() for n in nodes())
        print("PASS unvisited background session reports status without attach or size votes; project groups are distinct", flush=True)
        report(1, "background", "done")
        report(2, "blocked-a", "blocked")
        report(2, "blocked-b", "blocked")
        report(2, "finished", "done")
        report(3, "failed", "error")
        pump(lambda: any('"finished"' in m for m in messages()))
        assert not any(m.startswith("C ack") for m in messages())
        print("PASS complete background status sets arrive without implicit acknowledgement", flush=True)
        click("Status Records…")
        if os.environ.get("PIGEON_SIDEBAR_SCREENSHOT"):
            run("import", "-window", window, os.environ["PIGEON_SIDEBAR_SCREENSHOT"])
        select_record("blocked-a")
        assert not find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE)
        select_record("finished")
        pump(lambda: find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE))
        # Inspecting the record is not enough: its pane must remain live and visible.
        run("xdotool", "mousemove", "--window", window, "500", "600", "click", "1")
        key("shift+Page_Up")
        pump(lambda: not find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE))
        assert not any(m.startswith("C ack") for m in messages())
        key("shift+End")
        pump(lambda: find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE))
        run("xdotool", "mousemove", "--window", window, "1000", "550", "click", "1")
        key("ctrl+shift+z")
        pump(lambda: not find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE))
        key("ctrl+shift+z")
        pump(lambda: find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE))
        focus_window = Gtk.Window(title="Isolated sidebar focus target")
        focus_window.present()
        settle()
        other_id = run("xdotool", "search", "--onlyvisible", "--name", "Isolated sidebar focus target").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", other_id)
        settle()
        assert not find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE)
        run("xdotool", "windowfocus", "--sync", window)
        focus_window.destroy()
        settle()
        print("PASS scrollback, zoom-hidden pane and inactive window disable confirmation", flush=True)
        click("Mark This Record as Read")
        pump(lambda: any(m.startswith("C ack 2 ") for m in messages()))
        assert sum(m.startswith("C ack") for m in messages()) == 1
        print("PASS concrete record inspection and explicit done ack; blocked cannot ack", flush=True)
        # Clear invalidates the exact inspected identity; it must not select or ack a sibling.
        click("Status Records…")
        key("Home", "Down", "Down", "Down")
        command(3, "\x1b]7501;id=failed:state=clear\x07")
        pump(lambda: any(m.startswith("S status 3 2 ") and m.endswith("[]") for m in messages()))
        pump(lambda: not find("Mark This Record as Read", "button").get_state_set().contains(Atspi.StateType.SENSITIVE))
        assert sum(m.startswith("C ack") for m in messages()) == 1
        print("PASS cleared record invalidates inspection without acknowledging a sibling", flush=True)
        run("xdotool", "mousemove", "--window", window, "245", "135")
        settle()
        click("Close Session…")
        click("Cancel")
        assert not any(m.startswith("C close") for m in messages())
        run("xdotool", "mousemove", "--window", window, "245", "135")
        settle()
        click("Close Session…")
        click("Close Session")
        pump(lambda: any(m == "C close 2" for m in messages()) and any(m == "C close 3" for m in messages()))
        run("xdotool", "windowfocus", "--sync", window)
        settle()
        key("h")
        pump(lambda: (temporary / "bytes-1").read_bytes() == b"acfh")
        assert not any(m == "C close 1" for m in messages())
        print("PASS close-session confirmation closes only its panes and restores remaining session", flush=True)
        app.terminate()
        app.wait(timeout=3)
        assert host.poll() is None
        app = spawn("client-again", [str(ROOT / "core/target/debug/pigeon")], env=env)
        window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
        run("xdotool", "windowfocus", "--sync", window)
        settle(1)
        key("i")
        pump(lambda: (temporary / "bytes-1").read_bytes() == b"acfhi")
        assert len(list(temporary.glob("bytes-*"))) == 4
        assert not any(m == "C attach 4" for m in messages())
        print("PASS client exit leaves host/PTY alive; fresh client reattaches existing session", flush=True)
        for log in logs:
            log.flush()
            log.seek(0)
            text = log.read()
            assert "panicked" not in text and "CRITICAL" not in text, text
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try: child.wait(timeout=3)
                except subprocess.TimeoutExpired: child.kill(); child.wait()
        if sys.exc_info()[0]:
            for log in logs:
                log.flush(); log.seek(0)
                print(f"--- {log.name} ---\n{log.read()[-9000:]}", file=sys.stderr)
            if wire.exists(): print(wire.read_text()[-15000:], file=sys.stderr)
        for log in logs: log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv:
        check()
    else:
        build_proxy()
        input_check.__file__ = __file__
        sys.exit(input_check.launch())
