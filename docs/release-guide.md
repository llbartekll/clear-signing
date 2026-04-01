# Release Guide

This document describes the current release workflows as implemented in:
- [.github/workflows/release-swift.yml](../.github/workflows/release-swift.yml)
- [.github/workflows/release-kotlin.yml](../.github/workflows/release-kotlin.yml)

It intentionally documents the repo as it exists today, including the distinction between local development packaging and published release packaging.

## Shared Versioning

- Swift and Kotlin releases use the same semver tag, such as `0.1.0`.
- The Swift workflow creates the git tag.
- The Kotlin workflow uploads Android artifacts to the GitHub Release for that tag.

Recommended order:
1. Run the Swift release workflow first.
2. Run the Kotlin release workflow second.

## Swift Release

Workflow:
- `Release Swift (iOS)`
- Trigger: manual `workflow_dispatch`
- Input: `version`

What the workflow does:
1. Checks out the repo with full git history.
2. Installs the Rust toolchain with Apple targets:
   - `aarch64-apple-ios`
   - `x86_64-apple-ios`
   - `aarch64-apple-ios-sim`
3. Runs [scripts/build-xcframework.sh](../scripts/build-xcframework.sh).
4. Zips `target/ios/libclear_signing.xcframework` into `Output/libclear_signing.xcframework.zip`.
5. Computes the Swift package checksum with `swift package compute-checksum`.
6. Rewrites [Package.swift](../Package.swift):
   - release URL changed to `/download/<version>/libclear_signing.xcframework.zip`
   - `checksum` updated to the newly computed value
7. Commits the `Package.swift` change to `main`.
8. Creates and pushes the git tag.
9. Creates a GitHub Release and uploads `Output/libclear_signing.xcframework.zip`.

Current packaging note:
- The checked-in `Package.swift` defaults to the release XCFramework URL.
- Local Swift development is enabled explicitly with `USE_LOCAL_RUST_XCFRAMEWORK=1`.
- The release workflow is what turns the manifest into a release-consumable package definition for the tagged version.

### Consumer Shape

Swift consumers integrate through the `ClearSigning` package product from the tagged repository release.

### Local Development vs Release

Local development:
- Build the XCFramework locally with `./scripts/build-xcframework.sh`.
- The package resolves against `target/ios/libclear_signing.xcframework`.

Published release:
- Use the tagged repository version after the workflow has updated `Package.swift` and attached the XCFramework zip to the GitHub Release.

## Kotlin Release

Workflow:
- `Release Kotlin (Android)`
- Trigger: manual `workflow_dispatch`
- Input: `version`

What the workflow does:
1. Checks out the repo.
2. Installs the Rust toolchain with Android targets:
   - `aarch64-linux-android`
   - `armv7-linux-androideabi`
   - `x86_64-linux-android`
3. Installs Java 17 and the Android SDK/NDK.
4. Installs `cargo-ndk`.
5. Builds `libclear_signing.so` for the three Android targets.
6. Generates Kotlin UniFFI bindings from the Android `.so` into `kotlin-bindings/`.
7. Strips the Android shared libraries.
8. Arranges release artifacts into:
   - `libs/arm64-v8a/`
   - `libs/armeabi-v7a/`
   - `libs/x86_64/`
   - `kotlin-bindings/`
9. Packages those files into `kotlin-artifacts.zip`.
10. Uploads `kotlin-artifacts.zip` to the GitHub Release for the specified tag.

### Consumer Shape

Android consumers currently integrate through JitPack:

```groovy
dependencyResolutionManagement {
    repositories {
        maven { url 'https://jitpack.io' }
    }
}

dependencies {
    implementation 'com.github.llbartekll:clear-signing:0.1.0'
}
```

Current model:
- The GitHub Release stores `kotlin-artifacts.zip`.
- JitPack builds the Android artifact from the repo and release assets on first consumer request.
- The Android library surface is the handwritten `com.clearsigning.ClearSigningClient` plus types exposed from `android/clear-signing`.

### Local Development vs Release

Local development:
- Use the same generated-artifact flow as CI, targeting `android/build/generated/clear-signing/`.
- Then run:

```sh
cd android
./gradlew :clear-signing:assembleRelease :clear-signing:publishReleasePublicationToMavenLocal
```

Published release:
- Use the GitHub Release tag plus the JitPack dependency coordinate.

## Troubleshooting

### Swift Workflow Fails

- If the workflow fails before the tag is pushed, fix the issue and rerun.
- If the workflow fails after pushing the tag or creating the release, clean up the tag/release before rerunning so the workflow can recreate them cleanly.
- If the XCFramework zip is missing or the checksum is wrong, inspect the `Build XCFramework`, `Zip XCFramework`, and `Compute checksum` steps.

### Kotlin Workflow Fails

- Verify the Android NDK installation succeeded and `cargo-ndk` built all three targets.
- If `kotlin-artifacts.zip` already exists on the GitHub Release, delete the existing asset before rerunning the workflow.
- If binding generation fails, inspect the `Generate Kotlin bindings` step and verify the Android `.so` exists at the expected target path.

### JitPack Build Fails

- Inspect the build logs on JitPack for the requested version.
- Confirm the GitHub Release for that tag includes `kotlin-artifacts.zip`.
- Confirm the Android Gradle module still expects generated sources and `jniLibs` in the paths used by the repo workflows.
