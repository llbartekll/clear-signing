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
                url: "https://github.com/llbartekll/clear-signing/releases/download/0.0.1/libclear_signing.xcframework.zip",
                checksum: "1799b2e8afbc5f0237239793767fb9e700527aff10976773caafd3707554d77f"
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
