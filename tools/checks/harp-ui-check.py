#!/usr/bin/env python3
"""Capture the native UI in both appearances using an isolated host and real OSC reports."""
import argparse
import base64
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def capture(out):
    env = dict(os.environ, TERM="xterm-256color")
    with tempfile.TemporaryDirectory(prefix="pigeon-ui-", dir="/tmp") as temporary:
        work = Path(temporary)
        env.update(PIGEON_CONFIG_DIR=str(work / "config"), XDG_RUNTIME_DIR=str(work))
        (work / "config").mkdir()
        helper = work / "window-id"
        run("swiftc", str(ROOT / "tools/checks/helpers/window-id-of-pid.swift"), "-o", str(helper))
        host = app = None
        try:
            with (out / "host.log").open("w") as log:
                host = subprocess.Popen(
                    [str(ROOT / "core/target/debug/pigeon-server"), "--socket", str(work / "pigeon.sock"),
                     "--shell", "/bin/sh"], env=env, stdout=log, stderr=log)
            deadline = time.monotonic() + 5
            while not (work / "pigeon.sock").exists():
                if host.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("isolated host did not start")
                time.sleep(0.05)

            sessions = [
                ("launch", "landing-hero", "working", "Updating the landing page"),
                ("launch", "pricing-page", "blocked", "Choose monthly pricing: $12 or $15?"),
                ("launch", "launch-thread", "done", ""),
                ("launch", "og-images", "idle", ""),
                ("iOS app", "onboarding-v2", "working", "Refining the first-run experience"),
                ("iOS app", "paywall-crash", "blocked", "Allow the test run?"),
                ("iOS app", "app-store-shots", "done", ""),
                ("backend", "postgres-migration", "working", "Preparing the migration"),
                ("backend", "stripe-webhooks", "done", ""),
                ("backend", "rate-limiter", "idle", ""),
                ("other", "inbox-zero", "done", ""),
                ("other", "taxes-2026", "idle", ""),
                ("other", "dotfiles", "idle", ""),
            ]
            transcript = (
                "\n\n  Pricing table copy is in and the FAQ answers the three questions from support.\n\n"
                "  \033[90mWorked for 12m 04s\033[0m\n\n"
                "  Ship the new landing hero today. Split it up: copy, OG image, video.\n\n"
                "  \033[32m●\033[0m \033[1mRead\033[0m(src/components/Hero.tsx, src/styles/hero.css)\n"
                "    \033[90mRead 2 files (412 lines)\033[0m\n\n"
                "  \033[32m●\033[0m \033[1mTask\033[0m(spawn 3 background agents)\n"
                "    \033[90mhero: copy variants · hero: og image · hero: video render\033[0m\n\n"
                "  \033[32m●\033[0m \033[1mWrite\033[0m(src/components/Hero.tsx)\n"
                "    \033[90mWrote 86 lines\033[0m\n\n"
                "  \033[32m●\033[0m \033[1mBash\033[0m(pnpm build)\n"
                "    \033[90mCompiled in 4.2s · 0 errors · 0 warnings\033[0m\n\n"
                "  \033[32m●\033[0m \033[1mAgent hero: copy variants finished\033[0m\n"
                "    Wrote 6 headline variants. Ship while you sleep.\n\n"
                "  \033[33m●\033[0m \033[1mAgent pricing-page is waiting for you\033[0m\n"
                "    Two price points are ready. I need your call before wiring Stripe.\n"
            )
            for index, (group, name, state, message) in enumerate(sessions):
                cwd = work / "projects" / group
                cwd.mkdir(parents=True, exist_ok=True)
                status = f"state={state}:app={name}"
                if state == "blocked":
                    status += ":kind=permission"
                if message:
                    status += ":msg=" + base64.b64encode(message.encode()).decode()
                content = transcript if index == 0 else f"\n  {name}\n\n  {message}\n"
                report = f"\033]0;{name}\007\033]7501;{status}\007\033[?25l"
                # A fixture writes terminal bytes through a real PTY; no client-side VT parsing.
                shell = work / f"session-{index}.sh"
                shell.write_text("#!/bin/sh\nprintf '%s' " + shlex.quote(report)
                    + "\nwhile :; do\nprintf '%s' " + shlex.quote("\033[H\033[2J" + content)
                    + "\nsleep 2\ndone\n")
                shell.chmod(0o700)
                run(str(ROOT / "core/target/debug/examples/keys"), "--socket", str(work / "pigeon.sock"),
                    "--new-session", str(cwd), "--shell", str(shell), stdout=subprocess.DEVNULL)

            for appearance in ["light", "dark"]:
                env["PIGEON_APPEARANCE"] = appearance
                with (out / f"{appearance}.log").open("w") as log:
                    app = subprocess.Popen([str(ROOT / ".build/debug/pigeon")], env=env, stdout=log, stderr=log)
                deadline = time.monotonic() + 10
                window_id = ""
                while not window_id:
                    if app.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError("UI window did not appear")
                    window_id = run(str(helper), str(app.pid), capture_output=True, text=True).stdout.strip()
                    time.sleep(0.1)
                time.sleep(3.5)
                run("screencapture", "-x", "-o", "-l", window_id, str(out / f"{appearance}.png"))
                stop(app)
                app = None
        finally:
            stop(app)
            stop(host)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    capture(args.out.resolve())
    print(f"Screenshots: {args.out.resolve()}")
