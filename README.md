# Clear Signing SDK

Rust ERC-7730 v2 clear-signing library with SDK surfaces for Swift and Kotlin.

The repository contains:
- A Rust engine that resolves descriptors and formats contract calldata and EIP-712 typed data into a display model.
- A Swift package surface built on UniFFI bindings plus a handwritten `ClearSigningClient`.
- An Android/Kotlin SDK module built on UniFFI bindings plus a handwritten `ClearSigningClient`.
- Local demo and smoke-test consumers for validating the SDK integrations.

## What The SDK Does

- Formats calldata into a `DisplayModel` for wallet UI rendering.
- Formats EIP-712 typed data into the same display model shape.
- Resolves descriptors for direct calls and nested calldata flows.
- Supports proxy-aware descriptor resolution through wallet-provided `DataProviderFfi`.
- Delegates token, name, NFT, and block metadata lookups to the host wallet.

## SDK Surfaces

### Swift

- Package product: `ClearSigning`
- Integration style: Swift Package Manager
- Main API: `ClearSigningClient`
- Local development packaging: `target/ios/libclear_signing.xcframework`

See [docs/swift-integration.md](docs/swift-integration.md).

### Kotlin

- Android library module: `android/clear-signing`
- Published consumption: JitPack-backed Maven dependency
- Main API: `com.clearsigning.ClearSigningClient`
- Local development packaging: generated Kotlin bindings plus Android `jniLibs`

See [docs/kotlin-integration.md](docs/kotlin-integration.md).

## Release Docs

- [docs/release-guide.md](docs/release-guide.md)

## Local Development

Build and test the Rust crate from repo root:

```sh
cargo build
cargo test
cargo clippy -p clear-signing --all-targets --features uniffi,github-registry -- -D warnings
```

Swift local packaging:

```sh
./scripts/build-xcframework.sh
```

Android local packaging follows the same steps used in CI: build native libraries, generate Kotlin bindings, then assemble/publish the Android artifact. See the Kotlin integration guide for the exact local flow.

## Repo Notes

- The checked-in [Package.swift](Package.swift) is currently configured for local XCFramework development and currently declares `.iOS(.v16)`.
- The Swift release workflow rewrites `Package.swift` to point at the tagged release artifact and checksum during release.
- The Android SDK consumes generated bindings and native libraries from `android/build/generated/clear-signing/`.
