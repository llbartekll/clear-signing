# Clear Signing SDK

Rust ERC-7730 v2 clear-signing library with SDK surfaces for Swift and Kotlin.

The repository contains:
- A Rust engine that resolves descriptors and formats contract calldata and EIP-712 typed data into a display model.
- A Swift package surface built on UniFFI bindings plus a handwritten `ClearSigningClient`.
- An Android/Kotlin SDK module built on UniFFI bindings plus a handwritten `ClearSigningClient`.
- Local demo and smoke-test consumers for validating the SDK integrations.

## What The SDK Does

- Formats calldata and EIP-712 typed data into an explicit `FormatOutcome`:
  - `clearSigned`: descriptor-backed clear signing succeeded
  - `fallback`: renderable but degraded / unverified output
  - `failure`: invalid input, invalid descriptor, resolution outage, or internal failure
- Returns typed `FormatDiagnostic` entries for non-fatal degradation instead of string warnings.
- Resolves descriptors for direct calls and nested calldata flows via `DescriptorResolutionOutcome`.
- Supports proxy-aware descriptor resolution through wallet-provided `DataProviderFfi`.
- Delegates token, name, NFT, and block metadata lookups to the host wallet.

Provider callbacks are still best-effort in this phase:
- missing token or name metadata can produce diagnostics
- they do not automatically become hard failures

## SDK Surfaces

### Swift

- Package product: `ClearSigning`
- Integration style: Swift Package Manager
- Main API: `ClearSigningClient`
- Published release is the default package mode
- Local development packaging: `target/ios/libclear_signing.xcframework` when `USE_LOCAL_RUST_XCFRAMEWORK=1`

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
cargo check -p clear-signing --features uniffi,github-registry
cargo test -p clear-signing --features uniffi,github-registry
cargo clippy -p clear-signing --all-targets --features uniffi,github-registry -- -D warnings
```

Swift local packaging:

```sh
./scripts/generate_uniffi_bindings.sh
./scripts/build-xcframework.sh
USE_LOCAL_RUST_XCFRAMEWORK=1 swift package describe
xcodebuild -project wallet/Wallet.xcodeproj -scheme Wallet -destination 'generic/platform=iOS Simulator' build
```

Android local packaging follows the same steps used in CI: build native libraries, generate Kotlin bindings, then assemble/publish the Android artifact. See the Kotlin integration guide for the exact local flow.

## Repo Notes

- The checked-in [Package.swift](Package.swift) defaults to the release XCFramework URL and currently declares `.iOS(.v14)`.
- Set `USE_LOCAL_RUST_XCFRAMEWORK=1` to make SwiftPM resolve the local XCFramework at `target/ios/libclear_signing.xcframework`.
- The Swift release workflow rewrites `Package.swift` to point at the tagged release artifact and checksum during release.
- The Android SDK consumes generated bindings and native libraries from `android/build/generated/clear-signing/`.
