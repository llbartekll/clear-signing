# Release Guide

Both iOS (Swift/SPM) and Android (Kotlin/Maven) releases are automated via GitHub Actions workflows with manual dispatch.

## Versioning

Both platforms share the same semver version (e.g. `0.1.0`). Release Swift first when releasing both platforms.

## iOS (Swift/SPM)

### How to release

1. Go to **Actions → Release Swift (iOS) → Run workflow**
2. Enter the version (e.g. `0.1.0`)
3. The workflow:
   - Builds the XCFramework (arm64 device + arm64/x86_64 simulator)
   - Zips it and computes the Swift package checksum
   - Updates `Package.swift` with the new URL and checksum
   - Commits and pushes to main
   - Creates a git tag
   - Creates a GitHub Release with the XCFramework zip attached

### Consumer integration

```swift
// Package.swift
.package(url: "https://github.com/llbartekll/clear-signing", from: "0.1.0")
```

### Local development

Use the local XCFramework instead of the remote one:

```sh
USE_LOCAL_RUST_XCFRAMEWORK=1 swift build
```

## Android (Kotlin/Maven via JitPack)

### How to release

1. Go to **Actions → Release Kotlin (Android) → Run workflow**
2. Enter the version (e.g. `0.1.0`)
3. The workflow:
   - Cross-compiles native `.so` files for arm64-v8a, armeabi-v7a, x86_64
   - Generates Kotlin bindings via uniffi-bindgen
   - Strips debug symbols from `.so` files
   - Uploads `kotlin-artifacts.zip` to the GitHub Release

4. JitPack builds automatically on first consumer request, or trigger manually at `jitpack.io/#llbartekll/clear-signing`

### Consumer integration

```groovy
// settings.gradle
dependencyResolutionManagement {
    repositories {
        maven { url 'https://jitpack.io' }
    }
}

// build.gradle
dependencies {
    implementation 'com.github.llbartekll:clear-signing:0.1.0'
}
```

Kotlin consumers should integrate through the handwritten `com.clearsigning.ClearSigningClient`.

```kotlin
import com.clearsigning.ClearSigningClient

val client = ClearSigningClient(dataProvider)
val model = client.formatCalldata(
    chainId = 1uL,
    to = "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    calldataHex = "0xa9059cbb000000000000000000000000..."
)
```

## Recommended Release Order

1. **Swift first** — the Swift workflow creates the git tag and GitHub Release
2. **Kotlin second** — uploads artifacts to the existing release

Either platform can be released independently. If only Kotlin is released, it creates its own release (JitPack works from the release asset, not the tag content).

## How JitPack Works

- No credentials or secrets needed
- On first dependency request, JitPack downloads the pre-built `.so` files from the GitHub Release
- It runs `./gradlew assembleRelease` in the `android/` directory to build the AAR
- The AAR bundles handwritten Kotlin SDK classes from the repo together with generated UniFFI bindings and native `.so` files
- Build logs are visible at `jitpack.io/#llbartekll/clear-signing`

## Troubleshooting

### Swift workflow fails mid-release

- If it fails **before** the tag is created: fix and re-run, no cleanup needed
- If it fails **after** the tag: delete the tag (`git push --delete origin VERSION && git tag -d VERSION`), delete the draft release on GitHub, then re-run

### Kotlin workflow fails

- Re-run the workflow — it uploads to the existing release (or creates one if needed)
- If the release asset already exists, delete it from the GitHub Release page first

### JitPack build fails

- Check logs at `jitpack.io/#llbartekll/clear-signing` → select version → click "Log"
- Common issues: JDK version mismatch, missing artifacts zip, Gradle configuration errors
- To rebuild: delete the version on JitPack and re-request

### Cache invalidation

- Rust builds are cached via `Swatinem/rust-cache@v2` — cache is keyed by `Cargo.lock`
- To force a clean build, bump the cache key or clear the Actions cache
