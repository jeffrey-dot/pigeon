#!/usr/bin/env python3
"""Capture the GPUI client in both appearances using an isolated host and real OSC reports.

Mirrors tools/checks/harp-ui-check.py, but launches the single GPUI client
(`core/target/debug/pigeon-gpui`) instead of the AppKit app. A fixture writes terminal bytes
through a real PTY (no client-side VT parsing), and the same OSC 7501 sessions drive the
sidebar so the screenshot is comparable to docs/images/reference-harp-layout.png.

macOS only (uses `screencapture` and a CoreGraphics window-id helper). Needs the Metal
Toolchain to build the client; see docs/adr/0013-gpui-single-client.md.
"""
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


# The same fixture set as harp-ui-check.py, so the two clients are compared on identical data.
SESSIONS = [
    ("launchpad", "landing-hero", "working", "Updating the landing page"),
    ("launchpad", "pricing-page", "blocked", "Choose monthly pricing: $12 or $15?"),
    ("launchpad", "launch-thread", "done", ""),
    ("launchpad", "og-images", "idle", ""),
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

TRANSCRIPT = (
    "\n\n  Pricing table copy is in and the FAQ answers the three questions from support.\n\n"
    "  \033[90mWorked for 12m 04s\033[0m\n\n"
    "  Ship the new landing hero today. Split it up: copy, OG image, video.\n\n"
    "  \033[32m\u25cf\033[0m \033[1mRead\033[0m(src/components/Hero.tsx, src/styles/hero.css)\n"
    "    \033[90mRead 2 files (412 lines)\033[0m\n\n"
    "  \033[32m\u25cf\033[0m \033[1mTask\033[0m(spawn 3 background agents)\n"
    "    \033[90mhero: copy variants \u00b7 hero: og image \u00b7 hero: video render\033[0m\n\n"
    "  \033[32m\u25cf\033[0m \033[1mWrite\033[0m(src/components/Hero.tsx)\n"
    "    \033[90mWrote 86 lines\033[0m\n\n"
    "  \033[32m\u25cf\033[0m \033[1mBash\033[0m(pnpm build)\n"
    "    \033[90mCompiled in 4.2s \u00b7 0 errors \u00b7 0 warnings\033[0m\n\n"
    "  \033[33m\u25cf\033[0m \033[1mAgent pricing-page is waiting for you\033[0m\n"
    "    Two price points are ready. I need your call before wiring Stripe.\n"
)


def capture_window(helper, app, path, attempts=20):
    for _ in range(attempts):
        window_id = run(str(helper), str(app.pid), capture_output=True, text=True).stdout.strip()
        if window_id and subprocess.run(
                ["screencapture", "-x", "-o", "-l", window_id, str(path)],
                stderr=subprocess.DEVNULL).returncode == 0 and path.exists():
            return
        time.sleep(0.5)
    raise RuntimeError(f"could not capture the window into {path}")


# The reference shows minute-old working/done/idle reports and a just-raised blocked one. The
# fixture reproduces that with real time: most records are reported at once, blocked ones only
# BLOCKED_DELAY seconds later, and the first capture waits until AGE_WAIT seconds have passed.
AGE_WAIT = 64
BLOCKED_DELAY = 52

# A deep cwd for the minimum-size capture, so the header path has to truncate from the start.
NARROW_CWD = ("code", "clients", "acme", "platform", "monorepo", "services", "billing",
              "workers", "launchpad")


def start_host(runtime, env, out, name):
    # The client finds the host with `pigeon-server --socket-path`: $XDG_RUNTIME_DIR/pigeon.sock.
    socket = runtime / "pigeon.sock"
    log = (out / f"{name}-host.log").open("w")
    host = subprocess.Popen(
        [str(ROOT / "core/target/debug/pigeon-server"), "--socket", str(socket),
         "--shell", "/bin/sh"], env=env, stdout=log, stderr=log)
    deadline = time.monotonic() + 5
    while not socket.exists():
        if host.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError("isolated host did not start")
        time.sleep(0.05)
    return host, socket


def seed(work, socket, prefix, sessions, cwd_of, delay_blocked):
    for index, (group, name, state, message) in enumerate(sessions):
        cwd = cwd_of(group)
        cwd.mkdir(parents=True, exist_ok=True)
        status = f"state={state}:app={name}"
        if state == "blocked":
            status += ":kind=permission"
        if message:
            status += ":msg=" + base64.b64encode(message.encode()).decode()
        content = TRANSCRIPT if index == 0 else f"\n  {name}\n\n  {message}\n"
        title = f"\033]0;{name}\007\033[?25l"
        report = f"\033]7501;{status}\007"
        wait = f"sleep {delay_blocked}\n" if state == "blocked" and delay_blocked else ""
        # A fixture writes terminal bytes through a real PTY; no client-side VT parsing.
        shell = work / f"{prefix}-session-{index}.sh"
        shell.write_text(
            "#!/bin/sh\nprintf '%s' " + shlex.quote(title) + "\n" + wait
            + "printf '%s' " + shlex.quote(report)
            + "\nwhile :; do\nprintf '%s' " + shlex.quote("\033[H\033[2J" + content)
            + "\nsleep 2\ndone\n")
        shell.chmod(0o700)
        run(str(ROOT / "core/target/debug/examples/keys"), "--socket", str(socket),
            "--new-session", str(cwd), "--shell", str(shell), stdout=subprocess.DEVNULL)


def shoot(helper, env, out, name, size, runtime):
    env = dict(env, PIGEON_WINDOW_SIZE=size, XDG_RUNTIME_DIR=str(runtime))
    with (out / f"{name}.log").open("w") as log:
        app = subprocess.Popen([str(ROOT / "core/target/debug/pigeon-gpui")],
                               env=env, stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 15
        while not run(str(helper), str(app.pid), capture_output=True, text=True).stdout.strip():
            if app.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError(f"UI window did not appear ({name})")
            time.sleep(0.1)
        # Let the first frames (sessions, status, snapshot) land, then capture.
        time.sleep(4.0)
        capture_window(helper, app, out / f"{name}.png")
    finally:
        stop(app)


def capture(out, quick):
    env = dict(os.environ, TERM="xterm-256color")
    with tempfile.TemporaryDirectory(prefix="pigeon-gpui-ui-", dir="/tmp") as temporary:
        work = Path(temporary)
        # An isolated HOME keeps the run off the user's config and shows cwds as `~/code/...`,
        # the same shape as the reference header path.
        env.update(
            HOME=str(work),
            PIGEON_CONFIG_DIR=str(work / "config"),
            XDG_CONFIG_HOME=str(work / "config"),
            PIGEON_SERVER=str(ROOT / "core/target/debug/pigeon-server"),
        )
        (work / "config").mkdir()
        helper = work / "window-id"
        run("swiftc", str(ROOT / "tools/checks/helpers/window-id-of-pid.swift"),
            "-o", str(helper))
        hosts = []
        try:
            main_runtime = work / "main"
            narrow_runtime = work / "narrow"
            for runtime in (main_runtime, narrow_runtime):
                runtime.mkdir()
            host, socket = start_host(main_runtime, dict(env, XDG_RUNTIME_DIR=str(main_runtime)),
                                      out, "main")
            hosts.append(host)
            started = time.monotonic()
            seed(work, socket, "main", SESSIONS, lambda group: work / "code" / group,
                 0 if quick else BLOCKED_DELAY)
            host, socket = start_host(narrow_runtime,
                                      dict(env, XDG_RUNTIME_DIR=str(narrow_runtime)),
                                      out, "narrow")
            hosts.append(host)
            seed(work, socket, "narrow", SESSIONS[:1], lambda _: work.joinpath(*NARROW_CWD), 0)
            if not quick:
                time.sleep(max(0.0, AGE_WAIT - (time.monotonic() - started)))
            for appearance in ["light", "dark"]:
                shoot(helper, dict(env, PIGEON_APPEARANCE=appearance), out, appearance,
                      "1536x712", main_runtime)
            # Minimum window size: the header path must give way from its start (`…/launchpad`).
            shoot(helper, dict(env, PIGEON_APPEARANCE="light"), out, "narrow", "0x0",
                  narrow_runtime)
        finally:
            for host in hosts:
                stop(host)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("/tmp/pigeon-gpui"))
    parser.add_argument("--quick", action="store_true",
                        help="skip the minute of real time that ages the fixture reports")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    capture(args.out.resolve(), args.quick)
    print(f"Screenshots: {args.out.resolve()}")
    for name in ["light", "dark", "narrow"]:
        print(f"  {name}: {args.out.resolve() / (name + '.png')}")
