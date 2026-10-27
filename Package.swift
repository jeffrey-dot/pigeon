// swift-tools-version:6.0
import PackageDescription
import Foundation

// The core is a Rust static library built by cargo; point the linker at both
// profile directories so `swift build` works after either `cargo build` or
// `cargo build --release`.
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path

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
                .unsafeFlags([
                    "-L", "\(root)/core/target/debug",
                    "-L", "\(root)/core/target/release",
                    "-lpigeon_core",
                ])
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
