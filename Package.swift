// swift-tools-version:5.10
import Foundation
import PackageDescription

private let useLocalRustXCFramework = false

let package = Package(
    name: "ClearSigning",
    platforms: [
        .iOS(.v14)
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
                url: "https://github.com/llbartekll/clear-signing/releases/download/0.1.3/libclear_signing.xcframework.zip",
                checksum: "b219ea366092fe7c41c004af0a3417af6f0ffd364263aa6e97a9c5a5ddd1e06f"
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
