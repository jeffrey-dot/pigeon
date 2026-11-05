#!/usr/bin/env python3
"""Run a release host and API client in isolation; save raw protocol timing samples."""

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "tools/bench/host/Cargo.toml"
TARGET = ROOT / "core/target/host-bench"


def bounded(low, high):
    def parse(text):
        value = int(text)
        if not low <= value <= high:
            raise argparse.ArgumentTypeError(f"expected {low}..{high}")
        return value
    return parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=bounded(1, 10000), default=100)
    parser.add_argument("--panes", type=bounded(1, 16), default=4)
    parser.add_argument("--bytes-per-pane", type=bounded(1024, 64 * 1024 * 1024), default=1024 * 1024)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    subprocess.run(["cargo", "build", "--release", "--locked", "--bin", "pigeon-server"], cwd=ROOT, check=True)
    subprocess.run(["cargo", "build", "--release", "--locked", "--manifest-path", str(MANIFEST),
                    "--target-dir", str(TARGET)], cwd=ROOT, check=True)
    with (args.out / "environment.json").open("w") as file:
        subprocess.run(["python3", str(ROOT / "tools/bench/environment.py")], stdout=file, check=True)
    server = ROOT / "core/target/release/pigeon-server"
    client = TARGET / "release/pigeon-host-bench"
    hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in
              [("host", server), ("client", client), ("fixture", MANIFEST.parent / "fixture.py")]}
    # Short private paths work on both platforms' Unix socket length limits.
    with tempfile.TemporaryDirectory(prefix="pigeon-bench-", dir="/tmp") as private:
        private = pathlib.Path(private)
        (private / "config").mkdir()
        fixture = private / "fixture"
        python = shutil.which("python3")
        if python is None:
            raise RuntimeError("python3 required")
        fixture.write_bytes(f"#!{python}\n".encode() + (MANIFEST.parent / "fixture.py").read_bytes().split(b"\n", 1)[1])
        fixture.chmod(0o700)
        socket = private / "host.sock"
        env = {"HOME": str(private), "XDG_CONFIG_HOME": str(private / "config"),
               "SHELL": "/bin/sh", "PATH": "/usr/bin:/bin", "LANG": "C", "TERM": "dumb"}
        with (args.out / "host.log").open("w") as log:
            host = subprocess.Popen([str(server), "--socket", str(socket), "--shell", str(fixture)],
                                    env=env, cwd=private, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 10
                while not socket.exists():
                    if host.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError("host did not start; inspect host.log")
                    time.sleep(0.01)
                result = subprocess.run([str(client), str(socket), str(args.samples), str(args.panes),
                                         str(args.bytes_per_pane)], capture_output=True, text=True,
                                        timeout=180 + args.samples * 15 + args.panes * 30, check=False)
                (args.out / "client.log").write_text(result.stderr)
                result.check_returncode()
                report = json.loads(result.stdout)
                report.update(profile="release", transport="local Unix socket", binary_sha256=hashes,
                              measurement="send Keys through receipt and API application; excludes GUI display")
                (args.out / "samples.json").write_text(json.dumps(report, indent=2) + "\n")
                host.wait(timeout=5)
            finally:
                if host.poll() is None:
                    # Ask the owned host to kill its PTYs before stopping its runtime.
                    try:
                        subprocess.run([str(client), str(socket), "--shutdown"],
                                       stdout=subprocess.DEVNULL, stderr=log, timeout=6, check=False)
                    except subprocess.TimeoutExpired:
                        pass
                    try:
                        host.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        host.kill()
                        host.wait()
    print(f"Saved raw samples and metadata: {args.out.resolve()}")


if __name__ == "__main__":
    main()
