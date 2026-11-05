#!/usr/bin/env python3
"""Check documented direct dependencies and source import seams (AGENTS.md §3).

This is a source/manifest guard, not a Rust/Swift parser or transitive-license audit.
The compiler's privacy and unsafe lints and both platform builds remain required.
"""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
CORE_DEPENDENCIES = {
    "alacritty_terminal", "base64", "ciborium", "nix", "portable-pty", "serde",
    "serde_bytes", "tokio", "unicode-width",
}
HOST_ONLY = {"tokio", "portable-pty", "nix"}


def manifest_errors(packages):
    errors = []
    for package in packages:
        if package["name"] == "pigeon-core":
            for dep in package["dependencies"]:
                if dep["kind"] == "dev":
                    continue
                name = dep["name"]
                if name not in CORE_DEPENDENCIES:
                    errors.append(f"core: unreviewed direct dependency {name}")
                if dep["target"] is not None:
                    errors.append(f"core: target-specific dependency {name}")
                if name in HOST_ONLY and not dep["optional"]:
                    errors.append(f"core: {name} must be optional")
            features = package["features"]
            for name in HOST_ONLY:
                for feature, entries in features.items():
                    if feature != "host" and f"dep:{name}" in entries:
                        errors.append(f"core: {name} enabled outside host feature")
                if f"dep:{name}" not in features.get("host", []):
                    errors.append(f"core: host must enable {name}")
        if "/clients/" in package["manifest_path"]:
            for dep in package["dependencies"]:
                if dep["name"] == "pigeon-core" and (
                    dep["uses_default_features"] or {"internals", "host"}.intersection(dep["features"])
                ):
                    errors.append(f"{package['name']}: core dependency exposes host/internals")
    return errors


def source_errors(relative, source):
    errors = []
    # Remove comments first; imports in documentation are not program dependencies.
    code = re.sub(r"/\*.*?\*/|//[^\n]*", "", source, flags=re.S)
    if relative.startswith("core/src/"):
        if "/bin/" not in relative and re.search(r"\b(tokio|portable_pty)\s*::|\b(?:extern\s+crate|use)\s+(tokio|portable_pty)\b", code):
            errors.append("host runtime/PTY reference in portable library")
        if re.search(r"\btarget_os\s*=", code):
            errors.append("OS-specific branch needs a documented PTY/signal exception")
    if relative.endswith(".swift"):
        imports = re.findall(r"\bimport\s+(?:(?:class|struct|enum|protocol|func|var|typealias)\s+)?([A-Za-z_][A-Za-z_0-9]*)", code)
        if relative.startswith("clients/macos/Sources/PigeonCore/") and any(
            name in {"AppKit", "SwiftUI", "CoreText", "Metal", "MetalKit"} for name in imports
        ):
            errors.append("UI framework import in PigeonCore adapter")
        if "CoreText" in imports and relative != "clients/macos/Sources/PigeonApp/Terminal/CoreTextRenderer.swift":
            errors.append("CoreText import outside CoreTextRenderer.swift")
    return [f"{relative}: {error}" for error in errors]


def check(root):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"], cwd=root, text=True))
    errors = manifest_errors(metadata["packages"])
    for directory, pattern in [("core/src", "*.rs"), ("clients/macos/Sources", "*.swift")]:
        for path in (root / directory).rglob(pattern):
            errors.extend(source_errors(path.relative_to(root).as_posix(), path.read_text()))
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    errors = check(args.root.resolve())
    print("\n".join(errors) or "Architecture: direct dependencies and import seams checked")
    return bool(errors)


if __name__ == "__main__":
    sys.exit(main())
