# Release Guide

This document describes the release workflows as implemented in:

- [.github/workflows/release-all.yml](../.github/workflows/release-all.yml) — orchestrator for the SDK bindings (Swift + Kotlin + React Native)
- [.github/workflows/release-swift.yml](../.github/workflows/release-swift.yml) — Swift Package + XCFramework → GitHub Release
- [.github/workflows/release-kotlin.yml](../.github/workflows/release-kotlin.yml) — Kotlin/Android artifacts → GitHub Release (consumed via JitPack)
- [.github/workflows/release-react-native.yml](../.github/workflows/release-react-native.yml) — React Native package → npm
- [.github/workflows/release-crate.yml](../.github/workflows/release-crate.yml) — Rust crate → crates.io (standalone, not part of the orchestrator)

It documents the repo as it exists today, including local-development packaging vs. published packaging.

## Shared Versioning

- Swift, Kotlin, and React Native share one semver tag: `<version>` (e.g. `0.1.0`).
- The Swift workflow creates that git tag and the corresponding GitHub Release.
- The Kotlin and React Native workflows upload assets to that same release.
- The Crate workflow uses a separate tag namespace: `crate-<version>` (e.g. `crate-0.1.0`). It is released on its own cadence.

## Unified Release (recommended for SDK bindings)

Workflow:

- `Release All Platforms`
- Trigger: manual `workflow_dispatch`
- Input: `version` (strict semver `^[0-9]+\.[0-9]+\.[0-9]+$`, e.g. `0.1.0`)
- Scope: Swift + Kotlin + React Native. Cargo is intentionally excluded — see the Crate Release section below.

What it does:

1. Validates the `version` input against the strict semver regex up front.
2. Runs `swift` (depends only on validation).
3. After Swift succeeds, starts `kotlin` and `rn` in parallel — both `needs: swift`.
4. Each job calls the corresponding standalone workflow via `workflow_call`. Same code path as triggering each one manually.

Dependency graph:

```
validate
   └── swift
         ├── kotlin
         └── rn
```

Failure semantics:

- `swift` failure → `kotlin`/`rn` are skipped automatically (their tag check would fail anyway).
- `kotlin` failure → `swift`/`rn` are unaffected.
- `rn` failure → `swift`/`kotlin` already completed independently.

The orchestrator's overall run status reflects the worst child outcome — any single failure shows the run as red. Successful jobs are still done and their releases live.

### Recovering from a partial failure

When the orchestrator finishes red:

1. Open the run in the Actions UI and identify which child job(s) failed.
2. For each failed platform, re-trigger its **standalone** workflow (`release-swift.yml`, `release-kotlin.yml`, or `release-react-native.yml`) via `workflow_dispatch` with the same `version` input.
3. The Swift workflow refuses to overwrite an existing `<version>` tag — if it succeeded once already, don't re-run it.
4. Kotlin and RN re-runs upload to the existing GitHub Release. If the asset is already attached from a previous attempt, delete it from the release first to let the upload step replace it cleanly.

### When to skip the orchestrator

Use the standalone workflows directly when:

- You only want to ship one platform (e.g. RN-only fix).
- A previous orchestrator run partially succeeded and you're recovering specific platforms.

The Crate workflow always runs standalone — it's never part of `release-all.yml`.

## Crate Release

Workflow:

- `Release Crate (crates.io)`
- Trigger: `workflow_dispatch` (manual)
- Input: `version`
- Required secret: `CARGO_REGISTRY_TOKEN`

Cargo is intentionally not part of `release-all.yml` — it requires a manifest bump on `main` first, which doesn't fit the orchestrator's tag-driven release model.

What the workflow does:

1. Checks out the repo.
2. Fails early if the `crate-<version>` tag already exists on `origin`.
3. Asserts that `crates/clear-signing/Cargo.toml` `version` exactly matches the input.
4. Installs the Rust toolchain.
5. Runs `cargo publish -p clear-signing` against crates.io.
6. Tags the release as `crate-<version>` and pushes the tag.
7. Creates a GitHub Release for that tag with auto-generated release notes.

Required preparation: bump `crates/clear-signing/Cargo.toml` `version` and merge to `main` before triggering.

## Swift Release

Workflow:

- `Release Swift (iOS)`
- Trigger: `workflow_dispatch` (manual) or `workflow_call` (from `release-all.yml`)
- Input: `version`
- Note: when invoked via the orchestrator, this is the first job to run; Kotlin and RN gate on its success.

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
- Trigger: `workflow_dispatch` (manual) or `workflow_call` (from `release-all.yml`)
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

- The Swift release workflow must run first so the tag and GitHub Release exist.
- Kotlin attaches Android artifacts to that existing tag.

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

## React Native Release

Workflow:

- `Release React Native (iOS + Android)`
- Trigger: `workflow_dispatch` (manual) or `workflow_call` (from `release-all.yml`)
- Input: `version` (validated against strict semver `^[0-9]+\.[0-9]+\.[0-9]+$`)
- Required secret: `NPM_TOKEN`
- Concurrency group: `release-react-native-<version>` with `cancel-in-progress: false`

What the workflow does:

1. Validates the version regex.
2. Checks out the repo at the existing `<version>` tag (not `main`).
3. Confirms the GitHub Release for `<version>` already has `libclear_signing.xcframework.zip` attached. Refuses to proceed otherwise (Swift release must run first).
4. Installs the Rust toolchain with iOS + Android targets, Java 17, the Android SDK, and a pinned NDK (`27.0.12077973`) via `sdkmanager`.
5. Installs `cargo-ndk` (pinned version).
6. Regenerates iOS JSI bindings via `npx ubrn build ios --release --and-generate`.
7. Re-applies the local-dev podspec shape via `scripts/prepare-rn-podspec.sh`.
8. **iOS drift guard**: regeneration must produce zero diff against the committed bindings under `bindings/react-native/{cpp,ios,src/generated,react-native-clear-signing.podspec}`.
9. Regenerates Android JSI bindings + cross-compiled `.a` static archives via `npx ubrn build android --release --and-generate`.
10. Re-applies the 16 KB common-page-size CMake patch via `scripts/prepare-rn-android-cmake.sh`.
11. **Android drift guard**: regeneration must produce zero diff against the committed bindings under `bindings/react-native/{cpp,src/generated,android}` (excluding `android/src/main/jniLibs/`).
12. Strips the three Android `.a` files with `llvm-strip --strip-debug`.
13. Zips the RN-flavored XCFramework into `Output/libclear_signing-rn.xcframework.zip`.
14. Computes its SHA-256 checksum.
15. Uploads the zipped XCFramework to the GitHub Release for `<version>`.
16. Re-runs `scripts/prepare-rn-podspec.sh` in release mode with the version + checksum, baking a `prepare_command` into the podspec that downloads + verifies the asset on `pod install`.
17. Bumps `bindings/react-native/package.json` `version` (no git tag).
18. **npm tarball audit**: `npm pack --dry-run --json` must show compressed size ≤ 60 MB and all three Android ABI `.a` files present.
19. Publishes to npm as `react-native-clear-signing` with `--access public --provenance`.

### Consumer Shape

```sh
npm install react-native-clear-signing uniffi-bindgen-react-native
```

iOS adds `pod 'uniffi-bindgen-react-native', :path => '../node_modules/uniffi-bindgen-react-native'` to the Podfile and runs `bundle exec pod install`. Android picks up the package via autolinking; consumer's gradle/CMake links the bundled `.a` files with the JSI bridge during the first build.

See [docs/react-native-integration.md](react-native-integration.md) for the full integration guide (install, `DataProviderFfi` skeleton, API usage, returned types, error shape) and [bindings/react-native/README.md](../bindings/react-native/README.md) for the npm-package quickstart.

### Local Development vs Release

Local development:

- Use [bindings/react-native/example/](../bindings/react-native/example) — bare RN 0.81.5 app consuming the package via `file:..`. Mirrors the iOS Wallet Debug tab.
- Regenerate bindings after Rust changes with `./scripts/build-rn-ios.sh` and `./scripts/build-rn-android.sh`. The XCFramework + `jniLibs/` are gitignored.

Published release:

- Consumers `npm install react-native-clear-signing@<version>`. The podspec downloads + checksum-verifies the XCFramework from the matching GitHub Release on first `pod install`.

## Troubleshooting

### Orchestrator Failures

- The `Release All Platforms` run shows red whenever any child job fails. Click into the run and inspect the `swift`/`kotlin`/`rn` jobs to see which one failed.
- Skipped jobs (yellow/grey) downstream of a failed Swift job are normal — re-run Swift first, then re-run Kotlin and RN standalone.
- Re-runs go through the standalone workflows (`release-X.yml` with `workflow_dispatch`), not by re-triggering `release-all.yml` (which would try to re-publish anything that already succeeded).

### Crate Workflow Fails

- If `Cargo.toml` version doesn't match the input, fix the manifest on `main` first then re-trigger.
- If `cargo publish` rejects with "already exists", the version was previously published — bump `Cargo.toml` and use a new version.
- If the workflow tagged but failed afterwards, delete the `crate-<version>` tag and the GitHub Release before re-running.

### Swift Workflow Fails

- If the workflow fails before the tag is pushed, fix the issue and rerun.
- If the workflow fails after pushing the tag or creating the release, clean up the tag/release before rerunning so the workflow can recreate them cleanly.
- If the XCFramework zip is missing or the checksum is wrong, inspect the `Build XCFramework`, `Zip XCFramework`, and `Compute checksum` steps.

### Kotlin Workflow Fails

- Verify the Android NDK installation succeeded and `cargo-ndk` built all three targets.
- If `kotlin-artifacts.zip` already exists on the GitHub Release, delete the existing asset before rerunning the workflow.
- If binding generation fails, inspect the `Generate Kotlin bindings` step and verify the Android `.so` exists at the expected target path.

### React Native Workflow Fails

- "Release X exists but is missing libclear_signing.xcframework.zip" → re-run the Swift workflow first. RN cannot proceed without the iOS asset on the release.
- iOS or Android drift guard fails → regenerate locally with `scripts/build-rn-ios.sh` / `scripts/build-rn-android.sh`, commit the regenerated bindings, and trigger the workflow at the same version (it'll check out the new tag).
- npm tarball audit fails (>60 MB or missing ABI) → likely a Rust crate dependency change ballooned the static archives. Investigate before relaxing the ceiling.
- npm publish fails on auth → confirm the `NPM_TOKEN` repo secret is valid for the `react-native-clear-signing` package and has publish permission.
- npm publish fails on "version already exists" → the version was previously published. Bump and re-tag before re-trying.

### JitPack Build Fails

- Inspect the build logs on JitPack for the requested version.
- Confirm the GitHub Release for that tag includes `kotlin-artifacts.zip`.
- Confirm the Android Gradle module still expects generated sources and `jniLibs` in the paths used by the repo workflows.
