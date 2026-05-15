#!/usr/bin/env bash
# Local-dev convenience: regenerate the React Native package's Android bindings.
#
# Runs `ubrn build android --and-generate` to produce the Android TurboModule
# scaffolding (build.gradle, CMakeLists.txt, cpp-adapter.cpp, AndroidManifest,
# Kotlin sources) plus the cross-compiled Rust `.a` static archives for each
# ABI under android/src/main/jniLibs/. Then re-applies the 16 KB common-page-
# size linker patch via prepare-rn-android-cmake.sh so the committed
# CMakeLists.txt matches what gradle will see at consumer build time.
#
# Requires Android NDK and cargo-ndk to be installed locally.
#
# The npm tarball ships the `.a` files; the consumer's gradle/CMake links
# them with the JSI cpp adapter at the consumer's build time to produce
# `react-native-clear-signing.so`. The release CI workflow runs the same
# `ubrn build android` + patch sequence.

set -euo pipefail

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  echo "ERROR: \$ANDROID_NDK_HOME is not set." >&2
  echo "Install the pinned release NDK and export the path:" >&2
  echo "  sdkmanager \"ndk;27.0.12077973\"" >&2
  echo "  export ANDROID_NDK_HOME=\$ANDROID_HOME/ndk/27.0.12077973" >&2
  exit 1
fi

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "ERROR: cargo-ndk is not installed." >&2
  echo "Install with:  cargo install cargo-ndk" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_DIR="$REPO_ROOT/bindings/react-native"

cd "$PKG_DIR"

echo "==> installing npm deps (npm ci)"
npm ci

echo "==> running ubrn build android (NDK=$ANDROID_NDK_HOME)"
npx ubrn build android --and-generate --config ubrn.config.yaml

echo "==> applying 16 KB common-page-size CMake patch"
"$SCRIPT_DIR/prepare-rn-android-cmake.sh"

echo "RN Android package rebuilt at $PKG_DIR"
echo "  jniLibs:"
find android/src/main/jniLibs -name '*.a' 2>/dev/null | sort
