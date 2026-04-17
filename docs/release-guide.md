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
2. Fails early if the requested tag already exists on `origin`.
3. Installs the Rust toolchain with Apple targets:
   - `aarch64-apple-ios`
   - `x86_64-apple-ios`
   - `aarch64-apple-ios-sim`
4. Runs [scripts/build-xcframework.sh](../scripts/build-xcframework.sh).
5. Zips `target/ios/libclear_signing.xcframework` into `Output/libclear_signing.xcframework.zip`.
6. Computes the Swift package checksum with `swift package compute-checksum`.
7. Rewrites [Package.swift](../Package.swift):
   - flips `useLocalRustXCFramework` to `false`
   - release URL changed to `/download/<version>/libclear_signing.xcframework.zip`
   - `checksum` updated to the newly computed value
8. Validates the rewritten manifest semantically with `swift package dump-package`.
9. Commits the rewritten `Package.swift` on the release commit only.
10. Creates and pushes the git tag.
11. Creates a GitHub Release and uploads `Output/libclear_signing.xcframework.zip`.

Current packaging note:
- The checked-in `Package.swift` on `main` defaults to the local XCFramework path.
- The tagged release commit rewrites the manifest to the remote XCFramework URL and checksum.
- The release workflow is what turns the manifest into a release-consumable package definition for the tagged version without updating `main`.

### Consumer Shape

Swift consumers integrate through the `ClearSigning` package product from the tagged repository release.

### Local Development vs Release

Local development:
- Build the XCFramework locally with `./scripts/build-xcframework.sh`.
- On `main`, the package resolves against `target/ios/libclear_signing.xcframework`.

Published release:
- Use the tagged repository version after the workflow has rewritten `Package.swift` for that tag and attached the XCFramework zip to the GitHub Release.

## Kotlin Release

Workflow:
- `Release Kotlin (Android)`
- Trigger: manual `workflow_dispatch`
- Input: `version`

What the workflow does:
1. Checks out the repo.
2. Fails early unless the requested tag already exists on `origin`.
3. Installs the Rust toolchain with Android targets:
   - `aarch64-linux-android`
   - `armv7-linux-androideabi`
   - `x86_64-linux-android`
4. Installs Java 17 and the Android SDK/NDK.
5. Installs `cargo-ndk`.
6. Builds `libclear_signing.so` for the three Android targets.
7. Generates Kotlin UniFFI bindings from the Android `.so` into `kotlin-bindings/`.
8. Strips the Android shared libraries.
9. Arranges release artifacts into:
   - `libs/arm64-v8a/`
   - `libs/armeabi-v7a/`
   - `libs/x86_64/`
   - `kotlin-bindings/`
10. Packages those files into `kotlin-artifacts.zip`.
11. Uploads `kotlin-artifacts.zip` to the GitHub Release for the specified tag.

Release ordering note:
- Run the Swift release workflow first so it creates the tag and release-consumable Swift manifest.
- Run the Kotlin release workflow second so it attaches Android artifacts to that existing tag.

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
