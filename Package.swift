// swift-tools-version:6.0
import PackageDescription
import Foundation

// The core is a Rust static library built by cargo; point the linker at both
// profile directories so `swift build` works after either `cargo build` or
// `cargo build --release`.
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
// `PIGEON_CORE_LIB` names one directory instead: the universal release build
// (`tools/bundle-app.sh`) links a two-architecture library from there.
let coreLibs = ProcessInfo.processInfo.environment["PIGEON_CORE_LIB"].map { [$0] }
    ?? ["\(root)/core/target/debug", "\(root)/core/target/release"]

let package = Package(
    name: "pigeon",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "pigeon", targets: ["PigeonApp"]),
    ],
    targets: [
        .target(
            name: "PigeonCore",
            path: "Sources/PigeonCore",
            linkerSettings: [
                .unsafeFlags(coreLibs.flatMap { ["-L", $0] } + ["-lpigeon_core"])
            ]
        ),
        .executableTarget(
            name: "PigeonApp",
            dependencies: ["PigeonCore"],
            path: "Sources/PigeonApp"
        ),
        .testTarget(
            name: "PigeonAppTests",
            dependencies: ["PigeonApp"],
            path: "Tests/PigeonAppTests"
        ),
    ],
    swiftLanguageModes: [.v5]
)
