#!/bin/sh
# Compile the search model and real C ABI adapter on Linux or macOS; excludes AppKit.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Sources/PigeonApp" "$work/Sources/PigeonCore/include" "$work/Tests/PigeonAppTests"
cp "$root/include/pigeon_core.h" "$work/Sources/PigeonCore/include/"
printf '#include "include/pigeon_core.h"\n' > "$work/Sources/PigeonCore/empty.c"
for file in TerminalSearch SearchWire; do
    cp "$root/clients/macos/Sources/PigeonApp/Search/$file.swift" "$work/Sources/PigeonApp/"
done
cp "$root/clients/macos/Sources/PigeonApp/Terminal/SelectionSpan.swift" "$work/Sources/PigeonApp/"
cp "$root/clients/macos/Sources/PigeonApp/Net/Connection/Wire.swift" "$work/Sources/PigeonApp/"
for file in TerminalSearchTests SearchWireTests; do
    cp "$root/clients/macos/Tests/PigeonAppTests/$file.swift" "$work/Tests/PigeonAppTests/"
done
cat > "$work/Package.swift" <<'SWIFT'
// swift-tools-version:6.0
import PackageDescription
import Foundation
let coreLib = ProcessInfo.processInfo.environment["PIGEON_CORE_LIB"]!
let package = Package(
    name: "PigeonSearchChecks",
    targets: [
        .target(name: "PigeonCore", linkerSettings: [.unsafeFlags(["-L", coreLib, "-lpigeon_core"])]),
        .target(name: "PigeonApp", dependencies: ["PigeonCore"]),
        .testTarget(name: "PigeonAppTests", dependencies: ["PigeonApp", "PigeonCore"]),
    ],
    swiftLanguageModes: [.v5]
)
SWIFT
search_core_lib="${PIGEON_CORE_LIB:-$root/core/target/debug}"
PIGEON_CORE_LIB="$search_core_lib" LD_LIBRARY_PATH="$search_core_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "${SWIFT:-swift}" test --package-path "$work"
