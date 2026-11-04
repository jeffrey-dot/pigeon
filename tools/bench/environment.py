#!/usr/bin/env python3
"""Record reproducible benchmark metadata without dumping the user's environment."""

import datetime
import json
import os
import pathlib
import platform
import shutil
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[2]


def command(*args):
    if shutil.which(args[0]) is None:
        return {"available": False}
    try:
        result = subprocess.run(
            args, cwd=ROOT, text=True, capture_output=True, timeout=15, check=False
        )
        return {"exit_code": result.returncode, "output": result.stdout.strip()}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"error": type(error).__name__}


def linux_hardware():
    result = {}
    for name, path, keys in [
        ("cpu", "/proc/cpuinfo", ("model name", "Hardware")),
        ("memory", "/proc/meminfo", ("MemTotal",)),
    ]:
        file = pathlib.Path(path)
        if file.exists():
            for line in file.read_text().splitlines():
                key, _, value = line.partition(":")
                if key.strip() in keys:
                    result[name] = value.strip()
                    break
    if pathlib.Path("/etc/os-release").exists():
        result["os_release"] = pathlib.Path("/etc/os-release").read_text().splitlines()
    # PCI descriptions identify the GPU without exposing display-server sockets or EDID.
    pci = command("lspci")
    if "output" in pci:
        result["graphics"] = [
            line.partition(" ")[2]
            for line in pci["output"].splitlines()
            if any(kind in line for kind in ("VGA compatible", "3D controller", "Display controller"))
        ]
    return result


def main():
    system = platform.system()
    hardware = linux_hardware() if system == "Linux" else {}
    if system == "Darwin":
        hardware = {
            "cpu": command("sysctl", "-n", "machdep.cpu.brand_string"),
            "memory_bytes": command("sysctl", "-n", "hw.memsize"),
            "os_release": command("sw_vers"),
        }
    report = {
        "schema": 1,
        "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "system": system,
        "release": platform.release(),
        "architecture": platform.machine(),
        "logical_cpus": os.cpu_count(),
        "hardware": hardware,
        "revision": command("git", "rev-parse", "HEAD"),
        "worktree": command("git", "status", "--porcelain"),
        "rust": command("rustc", "-Vv"),
        "cargo": command("cargo", "--version"),
        "swift": command("swift", "--version"),
        "gtk": command("pkg-config", "--modversion", "gtk4"),
        "environment": {
            key: os.environ.get(key)
            for key in ("TERM", "COLORTERM", "XDG_SESSION_TYPE", "LANG")
        },
        # These affect GUI results but cannot be inferred from a headless collector.
        "manual_measurement_metadata_required": [
            "build profile", "GPU driver", "font and size", "grid and pane count",
            "display refresh rate and scale", "foreground or occluded", "workload",
        ],
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
