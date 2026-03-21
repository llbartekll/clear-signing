// swift-tools-version:5.10
import Foundation
import PackageDescription

let useLocal = ProcessInfo.processInfo.environment["USE_LOCAL_RUST_XCFRAMEWORK"] == "1"

let package = Package(
    name: "Erc7730",
    platforms: [
        .iOS(.v17)
    ],
    products: [
        .library(name: "Erc7730", targets: ["Erc7730"])
    ],
    targets: [
        useLocal
            ? .binaryTarget(
                name: "Erc7730Rust",
                path: "target/ios/liberc7730.xcframework"
            )
            : .binaryTarget(
                name: "Erc7730Rust",
                url: "https://github.com/llbartekll/lucid-umbrella/releases/download/0.1.0/liberc7730.xcframework.zip",
                checksum: "PLACEHOLDER"
            ),
        .target(
            name: "Erc7730",
            dependencies: ["Erc7730Rust"],
            path: "bindings/swift",
            exclude: ["erc7730FFI.h", "erc7730FFI.modulemap"],
            publicHeadersPath: "."
        )
    ]
)
