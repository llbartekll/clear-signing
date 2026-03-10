# ERC-7730 v2 Clear Signing Library

Guidance for GPT/Codex-style agents working in this repository.

## Project Summary

Rust library for ERC-7730 v2 clear signing. It decodes and formats contract calldata and EIP-712 messages for human-readable display.
The crate also provides UniFFI bindings (Kotlin + Swift) through a stateless FFI surface.

## Workspace Layout

- Cargo workspace root at `/`
- Single crate: `crates/erc7730/`
- Local Swift package manifest: `Package.swift`
- iOS demo app: `wallet/Wallet.xcodeproj`

## Build & Test

```sh
cargo build          # Build
cargo test           # Run default tests (34 unit + 12 integration)
cargo clippy         # Lint
cargo fmt --check    # Format check
```

UniFFI checks and binding generation:

```sh
cargo check -p erc7730 --features uniffi
cargo test -p erc7730 --features uniffi     # 41 unit tests + 12 integration
cargo clippy -p erc7730 --all-targets --features uniffi -- -D warnings
./scripts/generate_uniffi_bindings.sh
./scripts/build-xcframework.sh
swift package resolve
swift package describe
```

Generated binding outputs:
- `bindings/kotlin/uniffi/erc7730/erc7730.kt`
- `bindings/swift/erc7730.swift`
- `bindings/swift/erc7730FFI.h`
- `bindings/swift/erc7730FFI.modulemap`
- `target/ios/liberc7730.xcframework`

Repository policy:
- `bindings/swift/` is kept in-repo for SPM consumption.
- `bindings/kotlin/` is generated locally and gitignored.
- XCFramework is generated locally (not committed) and consumed by local `Package.swift`.
- Local Swift package and `wallet` app deployment baseline is iOS 14+.
- XCFramework header/modulemap staging is namespaced (`Headers/erc7730FFI/module.modulemap`) to avoid collisions with other Rust XCFrameworks.

## Code Conventions

- Rust 2021 edition
- Use `thiserror` for error types and `serde` for serialization
- Do not use `.unwrap()` in library code; propagate errors with `Result` and `?`
- Re-export all public API from `lib.rs`
- Decoding is signature-based: parse function signatures from descriptor format keys, without ABI JSON

## Public API

Four entry points, all in `lib.rs`:
- `format(chain_id, to, calldata, value, source, tokens)` — high-level: resolves descriptor then formats
- `format_calldata(descriptor, chain_id, to, calldata, value, tokens)` — low-level: format with pre-resolved descriptor
- `format_calldata_with_from(descriptor, chain_id, to, calldata, value, from, tokens)` — with `@.from` container value support
- `format_typed_data(descriptor, data, tokens)` — EIP-712 typed data formatting

UniFFI FFI exports in `src/uniffi_compat/mod.rs`:
- `erc7730_format_calldata(descriptor_json, chain_id, to, calldata_hex, value_hex, from_address, tokens)`
- `erc7730_format_typed_data(descriptor_json, typed_data_json, tokens)`

Local Swift package product:
- `Erc7730` (binary target + Swift wrapper target)

## Key Modules

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `engine.rs` | `DisplayModel`, `DisplayEntry`, `DisplayItem` | Main formatting pipeline |
| `decoder.rs` | `FunctionSignature`, `ParamType`, `ArgumentValue` | Calldata decoding from function signatures |
| `eip712.rs` | `TypedData`, `TypedDataDomain` | EIP-712 typed data support |
| `resolver.rs` | `DescriptorSource` (trait), `ResolvedDescriptor`, `StaticSource`, `FilesystemSource`, `GitHubRegistrySource` | Descriptor resolution (static, filesystem, HTTP) |
| `token.rs` | `TokenSource` (trait), `TokenMeta`, `WellKnownTokenSource`, `CompositeTokenSource` | Token metadata (CAIP-19 keys, embedded well-known tokens) |
| `address_book.rs` | `AddressBook` | Address-to-label resolution from descriptor metadata |
| `uniffi_compat/` | `TokenMetaInput`, `FfiError`, exported FFI functions | Stateless UniFFI wrapper layer |
| `types/` | `Descriptor`, `DescriptorContext`, `DescriptorDisplay`, `DisplayField`, `FieldFormat`, `VisibleRule` | Descriptor, display, context, and metadata types |
| `error.rs` | `Error`, `DecodeError`, `ResolveError` | Unified error hierarchy |
| `scripts/build-xcframework.sh` | XCFramework build + namespaced modulemap staging | iOS packaging for local SPM |
| `wallet/` | SwiftUI smoke-test app | Minimal consumer of local `Erc7730` package |

## V2 Registry Compatibility

The library supports v2 registry descriptor features:
- **Named parameter paths**: `"path": "amount"` resolved by parameter name from signature
- **`{paramName}` interpolation**: v2 intent syntax (alongside v1 `${path}`)
- **Threshold/message**: `"threshold": "$.metadata.constants.max"` + `"message": "All"` for max-amount display
- **`$ref` enum resolution**: `"$ref": "$.metadata.enums.interestRateMode"`
- **Container values**: `@.value`, `@.from`, `@.to`, `@.chainId` injected as synthetic arguments
- **Graceful degradation**: Unknown selectors return raw preview instead of errors
- **`duration`/`unit` formatters**: Seconds → human-readable, numeric + unit symbol

Optional features:
- `github-registry`: HTTP-based descriptor fetching via `GitHubRegistrySource` (adds `ureq` dependency)

## Working Expectations For Agents

- Prefer minimal, targeted changes over broad refactors
- Preserve existing public API unless the task explicitly requires API changes
- Add or update tests when changing formatting, decoding, or resolution behavior
- Run relevant Rust checks after changes when possible
- Keep docs and module exports aligned with implementation changes

## Pending

- **Phase 2**: `format_multi()` + `FieldFormat::Calldata` (nested calldata, Safe wallet support)
- **Phase 3**: `EmbeddedSource` + descriptor validation
- **Phase 4**: Packaging/distribution for existing UniFFI bindings (Swift XCFramework/SPM + Kotlin AAR/Maven)
- **Phase 5**: Missing formatter (`nftName`), file inclusion (`$id`/includes), CI pipeline
