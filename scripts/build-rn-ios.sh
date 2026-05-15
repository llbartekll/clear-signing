#!/usr/bin/env bash
# Local-dev convenience: regenerate the React Native package's iOS bindings.
#
# Runs `ubrn build ios --sim-only --and-generate` to produce the JSI bridge
# (cpp/), the TurboModule shim (ios/), the TypeScript bindings (src/generated/),
# and the local XCFramework (gitignored). Then re-applies our podspec patches
# via prepare-rn-podspec.sh so the file matches the committed shape.
#
# Run after editing the Rust crate or after a fresh checkout, before `pod install`
# in the example app.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_DIR="$REPO_ROOT/bindings/react-native"

cd "$PKG_DIR"

echo "==> installing npm deps (npm ci to honor the lockfile-pinned ubrn version)"
npm ci

echo "==> running ubrn build ios"
npx ubrn build ios --sim-only --and-generate --config ubrn.config.yaml

echo "==> applying podspec patches (local-dev mode)"
"$SCRIPT_DIR/prepare-rn-podspec.sh"

echo "RN package rebuilt at $PKG_DIR"
