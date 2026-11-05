#!/usr/bin/env python3
"""Capture the actual GTK UI with isolated real PTYs and explicit OSC fixture records."""
import importlib.util
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("input_check", Path(__file__).with_name("linux-input-check.py"))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def check():
    root = Path(os.environ["TEST_ROOT"])
    out = Path(os.environ.get("PIGEON_HARP_ARTIFACTS", "/tmp/pigeon-linux-harp"))
    out.mkdir(parents=True, exist_ok=True)
    server = ROOT / "core/target/debug/pigeon-server"
    socket = root / "runtime/pigeon.sock"
    children = []
    logs = []

    def spawn(name, command, env=None):
        log = (out / (name + ".log")).open("w")
        logs.append(log)
        child = subprocess.Popen(command, env=env, stdout=log, stderr=log)
        children.append(child)
        return child

    try:
        spawn("host", [str(server), "--socket", str(socket), "--shell", "/bin/sh"])
        deadline = time.monotonic() + 10
        while not socket.exists():
            if time.monotonic() > deadline: raise RuntimeError("host startup timeout")
            time.sleep(.05)
        sessions = [
            ("Launch", "landing-hero", "working"), ("Launch", "pricing-page", "blocked"),
            ("Launch", "launch-thread", "done"), ("Launch", "og-images", "idle"),
            ("iOS app", "onboarding-v2", "working"), ("iOS app", "paywall-crash", "blocked"),
            ("iOS app", "app-store-shots", "done"), ("Backend", "postgres-migration", "working"),
            ("Backend", "stripe-webhooks", "done"), ("Backend", "rate-limiter", "idle"),
            ("Other", "inbox-zero", "done"), ("Other", "taxes-2026", "idle"), ("Other", "dotfiles", "idle"),
        ]
        transcript = (
            "\r\n\r\nPricing table copy is in and the FAQ answers the three questions from support.\r\n\r\n"
            "\x1b[90m* Worked for 12m 04s · done 9:41 AM\x1b[0m\r\n\r\n"
            "› Ship the new landing hero today. Split it up: copy, OG image, video.\r\n\r\n"
            "\x1b[32m●\x1b[0m \x1b[1mRead\x1b[0m(src/components/Hero.tsx, src/styles/hero.css)\r\n"
            "   \x1b[90m└ Read 2 files (412 lines)\x1b[0m\r\n\r\n"
            "\x1b[32m●\x1b[0m \x1b[1mTask\x1b[0m(spawn 3 background agents)\r\n"
            "   \x1b[90m└ hero: copy variants · hero: og image · hero: video render\x1b[0m\r\n\r\n"
            "\x1b[32m●\x1b[0m \x1b[1mWrite\x1b[0m(src/components/Hero.tsx)\r\n"
            "   \x1b[90m└ Wrote 86 lines\x1b[0m\r\n\r\n"
            "\x1b[32m●\x1b[0m \x1b[1mBash\x1b[0m(pnpm build)\r\n"
            "   \x1b[90m└ ✓ Compiled in 4.2s · 0 errors · 0 warnings\x1b[0m\r\n\r\n"
            "\x1b[90m* Waiting for 3 background agents to finish\x1b[0m\r\n\r\n"
            "\x1b[32m●\x1b[0m \x1b[1mAgent hero: copy variants finished\x1b[0m · 3m 41s\r\n"
            "  Wrote 6 headline variants. Ship while you sleep.\r\n\r\n"
            "\x1b[33m◎\x1b[0m \x1b[1mAgent pricing-page is waiting for you\x1b[0m\r\n"
            "  Two price points are ready. I need your call before wiring Stripe.\r\n"
        )
        for index, (group, name, state) in enumerate(sessions):
            cwd = root / "projects" / group
            cwd.mkdir(parents=True, exist_ok=True)
            content = transcript if index == 0 else f"\r\n{name}\r\n"
            fixture = root / f"fixture-{index}"
            payload = f"\x1b]0;{name}\x07\x1b]7501;id=demo:state={state}:app=demo\x07\x1b[?25l"
            fixture.write_text("#!/bin/sh\nprintf '%s' " + shlex.quote(payload)
                + "\nwhile :; do printf '%s' " + shlex.quote("\x1b[H\x1b[2J" + content) + "; sleep 2; done\n")
            fixture.chmod(0o700)
            subprocess.run([str(ROOT / "core/target/debug/examples/keys"), "--socket", str(socket),
                            "--new-session", str(cwd), "--shell", str(fixture)], check=True)
        for appearance in ["light", "dark"]:
            env = dict(os.environ, PIGEON_SERVER=str(server), PIGEON_APPEARANCE=appearance, GTK_IM_MODULE="simple")
            app = spawn(appearance, [str(ROOT / "core/target/debug/pigeon")], env)
            window = base.run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid), "--name", "pigeon").splitlines()[0]
            base.run("xdotool", "windowsize", window, "1220", "760")
            base.run("xdotool", "windowfocus", "--sync", window)
            time.sleep(3)
            base.run("import", "-window", window, str(out / f"{appearance}.png"))
            if app.poll() is not None: raise RuntimeError("client exited")
            app.terminate(); app.wait(timeout=4)
        print(f"PASS real GTK light/dark captures: {out}")
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try: child.wait(timeout=3)
                except subprocess.TimeoutExpired: child.kill(); child.wait()
        for log in logs: log.close()


if __name__ == "__main__":
    if "--inside" in sys.argv: check()
    else:
        base.__file__ = __file__
        sys.exit(base.launch())
