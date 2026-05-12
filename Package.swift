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
                url: "https://github.com/llbartekll/clear-signing/releases/download/0.1.2/libclear_signing.xcframework.zip",
                checksum: "f986f897297dafa75b7dc7853673dd0e9f2aa7e41e95c6463004afb007ada26e"
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
