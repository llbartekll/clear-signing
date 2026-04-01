// swift-tools-version:5.10
import Foundation
import PackageDescription

private let useLocalRustXCFramework = {
    let value = ProcessInfo.processInfo.environment["USE_LOCAL_RUST_XCFRAMEWORK"]?
        .trimmingCharacters(in: .whitespacesAndNewlines)
        .lowercased()
    return ["1", "true", "yes"].contains(value)
}()

let package = Package(
    name: "ClearSigning",
    platforms: [
        .iOS(.v16)
    ],
    products: [
        .library(name: "ClearSigning", targets: ["ClearSigning"])
    ],
    targets: [
        useLocalRustXCFramework
            ? .binaryTarget(
                name: "ClearSigningRust",
                path: "target/ios/libclear_signing.xcframework"
            )
            : .binaryTarget(
                name: "ClearSigningRust",
                url: "https://github.com/llbartekll/clear-signing/releases/download/0.0.3/libclear_signing.xcframework.zip",
                checksum: "110c9a60fee7b563a4644902508a28e246a2870f252369d8c3936b7c9ffe7031"
            ),
        .target(
            name: "ClearSigning",
            dependencies: ["ClearSigningRust"],
            path: "bindings/swift",
            exclude: ["clearSigningFFI.h", "clearSigningFFI.modulemap"],
            publicHeadersPath: "."
        )
    ]
)
