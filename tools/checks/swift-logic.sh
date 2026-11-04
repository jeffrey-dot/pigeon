#!/bin/sh
# Run the existing Foundation-only sidebar tests without AppKit or the Rust dylib.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Sources/PigeonApp" "$work/Tests/PigeonAppTests"
for file in AgentInfo Grouping GitRoot SessionRef SessionRow SessionReducer SidebarModel PaneStatus SidebarStore StatusRecord StatusAcknowledgement HostStatusState StatusVisibility; do
    cp "$root/clients/macos/Sources/PigeonApp/Sidebar/$file.swift" "$work/Sources/PigeonApp/"
done
cp "$root/clients/macos/Sources/PigeonApp/Net/Host/HostConnection.swift" "$work/Sources/PigeonApp/"
cp "$root/clients/macos/Sources/PigeonApp/Net/Connection/Heartbeat.swift" "$work/Sources/PigeonApp/"
for file in TerminalCell TerminalSelection; do
    cp "$root/clients/macos/Sources/PigeonApp/Terminal/$file.swift" "$work/Sources/PigeonApp/"
done
cp "$root/clients/macos/Sources/PigeonApp/Terminal/Render/CellAttrs.swift" "$work/Sources/PigeonApp/"
for file in SessionReducerTests SessionRefTests AgentRowTests HostConnectionTests SelectionTests SidebarTests StatusPersistenceTests StatusVisibilityTests; do
    cp "$root/clients/macos/Tests/PigeonAppTests/$file.swift" "$work/Tests/PigeonAppTests/"
done
cat > "$work/Package.swift" <<'SWIFT'
// swift-tools-version:6.0
import PackageDescription
let package = Package(
    name: "PigeonLogicChecks",
    targets: [
        .target(name: "PigeonApp"),
        .testTarget(name: "PigeonAppTests", dependencies: ["PigeonApp"]),
    ],
    swiftLanguageModes: [.v5]
)
SWIFT
"${SWIFT:-swift}" test --package-path "$work"
