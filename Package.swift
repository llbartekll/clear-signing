// swift-tools-version:5.10
import Foundation
import PackageDescription

let useLocal = true //ProcessInfo.processInfo.environment["USE_LOCAL_RUST_XCFRAMEWORK"] == "1"

let package = Package(
    name: "ClearSigning",
    platforms: [
        .iOS(.v16)
    ],
    products: [
        .library(name: "ClearSigning", targets: ["ClearSigning"])
    ],
    targets: [
        useLocal
            ? .binaryTarget(
                name: "ClearSigningRust",
                path: "target/ios/libclear_signing.xcframework"
            )
            : .binaryTarget(
                name: "ClearSigningRust",
                url: "https://github.com/llbartekll/clear-signing/releases/download/0.0.2/libclear_signing.xcframework.zip",
                checksum: "410090d2b29ace5d4540b037d5c4c9fc517bf7222fc9d23885a9b23d07a27a80"
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
