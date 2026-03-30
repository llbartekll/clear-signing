#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/target/debug"
KOTLIN_OUT="$ROOT_DIR/bindings/kotlin"
SWIFT_OUT="$ROOT_DIR/bindings/swift"

case "$(uname -s)" in
    Darwin)
        LIB_PATH="$TARGET_DIR/libclear_signing.dylib"
        ;;
    Linux)
        LIB_PATH="$TARGET_DIR/libclear_signing.so"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        LIB_PATH="$TARGET_DIR/clear_signing.dll"
        ;;
    *)
        echo "Unsupported host OS: $(uname -s)" >&2
        exit 1
        ;;
esac

mkdir -p "$KOTLIN_OUT" "$SWIFT_OUT"
rm -rf \
    "$KOTLIN_OUT/uniffi/erc7730" \
    "$KOTLIN_OUT/uniffi/clear_signing" \
    "$SWIFT_OUT/clearSigningFFI.h" \
    "$SWIFT_OUT/clearSigningFFI.modulemap" \
    "$SWIFT_OUT/clear_signingFFI.h" \
    "$SWIFT_OUT/clear_signingFFI.modulemap"

echo "Building clear-signing with UniFFI feature..."
cargo build -p clear-signing --features uniffi,github-registry

if [[ ! -f "$LIB_PATH" ]]; then
    echo "Expected library not found at: $LIB_PATH" >&2
    echo "Available candidate files:" >&2
    find "$TARGET_DIR" -maxdepth 1 -name '*clear_signing*' -print >&2 || true
    exit 1
fi

echo "Generating Kotlin bindings to $KOTLIN_OUT"
cargo run -p clear-signing --features uniffi,github-registry --bin uniffi-bindgen -- generate --library "$LIB_PATH" --language kotlin --out-dir "$KOTLIN_OUT"

echo "Generating Swift bindings to $SWIFT_OUT"
cargo run -p clear-signing --features uniffi,github-registry --bin uniffi-bindgen -- generate --library "$LIB_PATH" --language swift --out-dir "$SWIFT_OUT"

if [[ -f "$SWIFT_OUT/clear_signingFFI.h" ]]; then
    mv "$SWIFT_OUT/clear_signingFFI.h" "$SWIFT_OUT/clearSigningFFI.h"
fi
if [[ -f "$SWIFT_OUT/clear_signingFFI.modulemap" ]]; then
    mv "$SWIFT_OUT/clear_signingFFI.modulemap" "$SWIFT_OUT/clearSigningFFI.modulemap"
fi
perl -0pi -e 's/clear_signingFFI/clearSigningFFI/g' "$SWIFT_OUT/clear_signing.swift"
sed -i '' 's/clear_signingFFI.h/clearSigningFFI.h/g' "$SWIFT_OUT/clearSigningFFI.modulemap"
sed -i '' 's/module clear_signingFFI/module clearSigningFFI/g' "$SWIFT_OUT/clearSigningFFI.modulemap"

echo "Done. Bindings generated in $ROOT_DIR/bindings"
