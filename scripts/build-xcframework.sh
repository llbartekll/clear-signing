#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="release"
PACKAGE_NAME="clear_signing"
FFI_MODULE="clearSigningFFI"
STAGING_DIR="$ROOT_DIR/target/uniffi-xcframework-staging"
IOS_OUT_DIR="$ROOT_DIR/target/ios"
FAT_SIM_LIB_DIR="$ROOT_DIR/target/ios-simulator-fat/$PROFILE"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "This script currently supports macOS only (required for xcodebuild/lipo)." >&2
    exit 1
fi

ensure_target() {
    local target="$1"
    if ! rustup target list --installed | grep -q "^${target}$"; then
        echo "Installing missing Rust target: $target"
        rustup target add "$target"
    fi
}

build_rust_libraries() {
    echo "Building Rust static libraries for Apple targets..."
    ensure_target "aarch64-apple-ios"
    ensure_target "x86_64-apple-ios"
    ensure_target "aarch64-apple-ios-sim"

    cargo build --lib --release --features uniffi,github-registry --target aarch64-apple-ios -p clear-signing
    cargo build --lib --release --features uniffi,github-registry --target x86_64-apple-ios -p clear-signing
    cargo build --lib --release --features uniffi,github-registry --target aarch64-apple-ios-sim -p clear-signing

    cargo build --release --features uniffi,github-registry -p clear-signing
}

generate_swift_bindings() {
    echo "Generating Swift UniFFI bindings..."
    rm -rf "$STAGING_DIR"
    mkdir -p "$STAGING_DIR"

    local host_library="$ROOT_DIR/target/$PROFILE/lib${PACKAGE_NAME}.dylib"
    if [[ ! -f "$host_library" ]]; then
        echo "Missing host library: $host_library" >&2
        exit 1
    fi

    cargo run -p clear-signing --features uniffi,github-registry --bin uniffi-bindgen -- generate \
        --library "$host_library" \
        --language swift \
        --out-dir "$STAGING_DIR"
}

patch_swift_for_swift6() {
    local swift_file="$STAGING_DIR/${PACKAGE_NAME}.swift"
    if [[ ! -f "$swift_file" ]]; then
        echo "Missing generated Swift file: $swift_file" >&2
        exit 1
    fi

    echo "Patching generated Swift for Swift 6 compatibility..."
    sed -i '' 's/init(bytes: \[UInt8\])/init(byteArray: [UInt8])/g' "$swift_file"
    sed -i '' 's/let rbuf = bytes\.withUnsafeBufferPointer/let rbuf = byteArray.withUnsafeBufferPointer/g' "$swift_file"
    sed -i '' 's/RustBuffer(bytes: writer)/RustBuffer(byteArray: writer)/g' "$swift_file"

    if [[ -f "$STAGING_DIR/${PACKAGE_NAME}FFI.h" ]]; then
        mv "$STAGING_DIR/${PACKAGE_NAME}FFI.h" "$STAGING_DIR/${FFI_MODULE}.h"
    fi
    if [[ -f "$STAGING_DIR/${PACKAGE_NAME}FFI.modulemap" ]]; then
        mv "$STAGING_DIR/${PACKAGE_NAME}FFI.modulemap" "$STAGING_DIR/${FFI_MODULE}.modulemap"
    fi
    if [[ -d "$STAGING_DIR/${PACKAGE_NAME}FFI" ]]; then
        mv "$STAGING_DIR/${PACKAGE_NAME}FFI" "$STAGING_DIR/${FFI_MODULE}"
        if [[ -f "$STAGING_DIR/${FFI_MODULE}/${PACKAGE_NAME}FFI.h" ]]; then
            mv "$STAGING_DIR/${FFI_MODULE}/${PACKAGE_NAME}FFI.h" "$STAGING_DIR/${FFI_MODULE}/${FFI_MODULE}.h"
        fi
        if [[ -f "$STAGING_DIR/${FFI_MODULE}/${PACKAGE_NAME}FFI.modulemap" ]]; then
            mv "$STAGING_DIR/${FFI_MODULE}/${PACKAGE_NAME}FFI.modulemap" "$STAGING_DIR/${FFI_MODULE}/${FFI_MODULE}.modulemap"
        fi
    fi

    perl -0pi -e 's/clear_signingFFI/clearSigningFFI/g' "$swift_file"
    if [[ -f "$STAGING_DIR/${FFI_MODULE}.modulemap" ]]; then
        sed -i '' 's/clear_signingFFI.h/clearSigningFFI.h/g' "$STAGING_DIR/${FFI_MODULE}.modulemap"
        sed -i '' 's/module clear_signingFFI/module clearSigningFFI/g' "$STAGING_DIR/${FFI_MODULE}.modulemap"
    fi
    if [[ -f "$STAGING_DIR/${FFI_MODULE}.h" ]]; then
        perl -0pi -e 's/clear_signingFFI/clearSigningFFI/g' "$STAGING_DIR/${FFI_MODULE}.h"
    fi
}

create_fat_simulator_lib() {
    echo "Creating fat simulator library..."
    mkdir -p "$FAT_SIM_LIB_DIR"

    lipo -create \
        "$ROOT_DIR/target/x86_64-apple-ios/$PROFILE/lib${PACKAGE_NAME}.a" \
        "$ROOT_DIR/target/aarch64-apple-ios-sim/$PROFILE/lib${PACKAGE_NAME}.a" \
        -output "$FAT_SIM_LIB_DIR/lib${PACKAGE_NAME}.a"
}

stage_namespaced_headers() {
    local platform="$1"
    local out_dir="$STAGING_DIR/$platform/Headers/$FFI_MODULE"
    mkdir -p "$out_dir"

    if [[ -d "$STAGING_DIR/$FFI_MODULE" ]]; then
        cp -R "$STAGING_DIR/$FFI_MODULE/." "$out_dir/"

        if [[ -f "$out_dir/${FFI_MODULE}.modulemap" ]]; then
            mv "$out_dir/${FFI_MODULE}.modulemap" "$out_dir/module.modulemap"
        fi
    else
        cp "$STAGING_DIR/${FFI_MODULE}.h" "$out_dir/${FFI_MODULE}.h"

        if [[ -f "$STAGING_DIR/${FFI_MODULE}.modulemap" ]]; then
            cp "$STAGING_DIR/${FFI_MODULE}.modulemap" "$out_dir/module.modulemap"
        elif [[ -f "$STAGING_DIR/module.modulemap" ]]; then
            cp "$STAGING_DIR/module.modulemap" "$out_dir/module.modulemap"
        else
            echo "Missing modulemap in UniFFI output staging directory" >&2
            exit 1
        fi
    fi

    if [[ ! -f "$out_dir/${FFI_MODULE}.h" || ! -f "$out_dir/module.modulemap" ]]; then
        echo "Header staging failed for $platform" >&2
        exit 1
    fi

    sed -i '' 's/clear_signingFFI.h/clearSigningFFI.h/g' "$out_dir/module.modulemap"
    sed -i '' 's/module clear_signingFFI/module clearSigningFFI/g' "$out_dir/module.modulemap"
}

build_xcframework() {
    echo "Building XCFramework..."
    rm -rf "$IOS_OUT_DIR"
    mkdir -p "$IOS_OUT_DIR"

    rm -rf "$STAGING_DIR/device" "$STAGING_DIR/simulator"
    stage_namespaced_headers "device"
    stage_namespaced_headers "simulator"

    xcodebuild -create-xcframework \
        -library "$ROOT_DIR/target/aarch64-apple-ios/$PROFILE/lib${PACKAGE_NAME}.a" \
        -headers "$STAGING_DIR/device/Headers" \
        -library "$FAT_SIM_LIB_DIR/lib${PACKAGE_NAME}.a" \
        -headers "$STAGING_DIR/simulator/Headers" \
        -output "$IOS_OUT_DIR/lib${PACKAGE_NAME}.xcframework"
}

copy_swift_wrapper() {
    echo "Refreshing committed Swift wrapper..."
    cp "$STAGING_DIR/${PACKAGE_NAME}.swift" "$ROOT_DIR/bindings/swift/${PACKAGE_NAME}.swift"
    cp "$STAGING_DIR/${FFI_MODULE}.h" "$ROOT_DIR/bindings/swift/${FFI_MODULE}.h"
    cp "$STAGING_DIR/${FFI_MODULE}.modulemap" "$ROOT_DIR/bindings/swift/${FFI_MODULE}.modulemap"
}

build_rust_libraries
generate_swift_bindings
patch_swift_for_swift6
create_fat_simulator_lib
build_xcframework
copy_swift_wrapper

echo "Done. XCFramework: $IOS_OUT_DIR/lib${PACKAGE_NAME}.xcframework"
