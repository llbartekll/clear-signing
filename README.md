# ERC-7730 UniFFI iOS Quickstart

This repository provides:
- Rust `erc7730` clear-signing library
- UniFFI Swift wrapper (`bindings/swift/erc7730.swift`)
- Local Swift Package (`Package.swift`) backed by an XCFramework
- A simple iOS demo app: `wallet/`

SPM/package baseline: iOS 14+.

## Prerequisites

- macOS with Xcode installed
- Rust toolchain (`rustup`)
- Installed iOS Rust targets:

```sh
rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
```

## Build XCFramework

From repository root:

```sh
./scripts/build-xcframework.sh
```

Expected output:
- `target/ios/liberc7730.xcframework`

This script also regenerates UniFFI Swift bindings and refreshes:
- `bindings/swift/erc7730.swift`

For host-only binding generation without Apple packaging, use `./scripts/generate_uniffi_bindings.sh`.

## Use via Local SPM

`Package.swift` defines:
- binary target: `target/ios/liberc7730.xcframework`
- Swift wrapper target in `bindings/swift`

You can consume it from local projects as product `Erc7730`.

## Run Wallet Demo

1. Build XCFramework first:

```sh
./scripts/build-xcframework.sh
```

2. Open the demo project:

```sh
open wallet/Wallet.xcodeproj
```

3. Build and run scheme `Wallet` on iOS simulator.
4. Tap **Run smoke test**. You should see an `OK:` status with formatted intent.

## Collision-Safety Note (Modulemap)

`build-xcframework.sh` stages FFI headers/modulemap under namespaced directories:
- `Headers/erc7730FFI/module.modulemap`

This avoids flat `Headers/module.modulemap` collisions when multiple Rust XCFrameworks are present in the same app.

## Troubleshooting

- `No such module 'Erc7730'`:
  - Run `./scripts/build-xcframework.sh` first.
  - Resolve package dependencies again in Xcode.
- Missing Rust iOS targets:
  - Run the `rustup target add ...` command from prerequisites.
- Module/header conflicts with other native libs:
  - Ensure XCFramework was produced by `scripts/build-xcframework.sh` (namespaced header staging).
