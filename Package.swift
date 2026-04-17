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
                url: "https://github.com/llbartekll/clear-signing/releases/download/0.0.4/libclear_signing.xcframework.zip",
                checksum: "d9d74d0f5d9200f4f0a3dd2269d0535a074e00794bc23d42fbad35f153465029"
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
