#!/bin/sh
# Builds pigeon and assembles Pigeon.app: the window (Contents/MacOS/pigeon) plus the
# server for this Mac next to it, which is exactly where HostLauncher looks for it.
#
# Builds for remote machines are not bundled: the app downloads the one a server needs from
# this version's GitHub release (`.github/workflows/release.yml`, `ServerDownload.swift`).
# The bundle version must therefore match a release tag (`v<version>`).
#
#   tools/bundle-app.sh              -> build/Pigeon.app
#   tools/bundle-app.sh /Applications -> installs it there
#   UNIVERSAL=1 tools/bundle-app.sh   -> one app for Apple silicon and Intel (releases)
set -eu
cd "$(dirname "$0")/.."
out=${1:-build}/Pigeon.app
version=$(sed -n 's/^version = "\(.*\)"/\1/p' core/Cargo.toml | head -1)

if [ "${UNIVERSAL:-}" = 1 ]; then
    # Releases are updated in place, so the app has to run on whichever Mac installed it.
    for triple in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --release --manifest-path core/Cargo.toml --target "$triple"
    done
    # One library with both slices, in its own directory so the linker cannot pick up a
    # single-architecture build from core/target/{debug,release} instead.
    fat=core/target/universal
    mkdir -p "$fat"
    lipo -create -output "$fat/libpigeon_core.a" \
        core/target/aarch64-apple-darwin/release/libpigeon_core.a \
        core/target/x86_64-apple-darwin/release/libpigeon_core.a
    lipo -create -output "$fat/pigeon-server" \
        core/target/aarch64-apple-darwin/release/pigeon-server \
        core/target/x86_64-apple-darwin/release/pigeon-server
    export PIGEON_CORE_LIB="$PWD/$fat"
    swift build -c release --arch arm64 --arch x86_64
    app=$(swift build -c release --arch arm64 --arch x86_64 --show-bin-path)/pigeon
    server=$fat/pigeon-server
else
    cargo build --release --manifest-path core/Cargo.toml
    swift build -c release
    app=.build/release/pigeon
    server=core/target/release/pigeon-server
fi

rm -rf "$out"
mkdir -p "$out/Contents/MacOS"
cp "$app" "$server" "$out/Contents/MacOS/"
cat >"$out/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>pigeon</string>
    <key>CFBundleIdentifier</key><string>dev.pigeon.app</string>
    <key>CFBundleName</key><string>Pigeon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# No Developer ID here: ad-hoc sign so Gatekeeper runs it locally and sees one identity.
codesign --force --sign - "$out/Contents/MacOS/pigeon-server" "$out" >/dev/null 2>&1 || true
echo "$out"
