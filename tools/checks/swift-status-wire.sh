#!/bin/sh
# Compile the real Foundation adapter against the generated ABI and Rust library.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Sources/PigeonApp" "$work/Sources/PigeonCore/include" "$work/Tests/PigeonAppTests"
cp "$root/include/pigeon_core.h" "$work/Sources/PigeonCore/include/"
printf '#include "include/pigeon_core.h"\n' > "$work/Sources/PigeonCore/empty.c"
for file in AgentInfo SidebarModel StatusRecord StatusSnapshot; do
    cp "$root/clients/macos/Sources/PigeonApp/Sidebar/$file.swift" "$work/Sources/PigeonApp/"
done
for file in Wire StatusSnapshot+Wire; do
    cp "$root/clients/macos/Sources/PigeonApp/Net/Connection/$file.swift" "$work/Sources/PigeonApp/"
done
cp "$root/clients/macos/Tests/PigeonAppTests/StatusSnapshotWireTests.swift" "$work/Tests/PigeonAppTests/"
for file in TerminalCell TerminalLinks TerminalLinks+Wire; do
    cp "$root/clients/macos/Sources/PigeonApp/Terminal/$file.swift" "$work/Sources/PigeonApp/"
done
cp "$root/clients/macos/Tests/PigeonAppTests/TerminalLinksWireTests.swift" "$work/Tests/PigeonAppTests/"
cat > "$work/Package.swift" <<'SWIFT'
// swift-tools-version:6.0
import PackageDescription
import Foundation
let coreLib = ProcessInfo.processInfo.environment["PIGEON_CORE_LIB"]!
let package = Package(
    name: "PigeonStatusWireChecks",
    targets: [
        .target(name: "PigeonCore", linkerSettings: [.unsafeFlags(["-L", coreLib, "-lpigeon_core"])]),
        .target(name: "PigeonApp", dependencies: ["PigeonCore"]),
        .testTarget(name: "PigeonAppTests", dependencies: ["PigeonApp", "PigeonCore"]),
    ],
    swiftLanguageModes: [.v5]
)
SWIFT
status_core_lib="${PIGEON_CORE_LIB:-$root/core/target/debug}"
PIGEON_CORE_LIB="$status_core_lib" LD_LIBRARY_PATH="$status_core_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "${SWIFT:-swift}" test --package-path "$work"
