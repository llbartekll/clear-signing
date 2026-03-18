# ERC-7730 v2 Clear Signing Library

Rust library for ERC-7730 v2 clear signing — decodes and formats contract calldata and EIP-712 messages for human-readable display.
UniFFI bindings (Kotlin + Swift) are implemented in the same crate via a stateless FFI wrapper.

## Workspace Layout

- Cargo workspace root at `/`
- Single crate: `crates/erc7730/`
- Local Swift package manifest: `Package.swift`
- iOS demo app: `wallet/Wallet.xcodeproj`

## Build & Test

```sh
cargo build          # Build
cargo test           # Run default tests (32 unit + 33 integration)
cargo clippy         # Lint
cargo fmt --check    # Format check
```

UniFFI checks and binding generation:

```sh
cargo check -p erc7730 --features uniffi,github-registry
cargo test -p erc7730 --features uniffi,github-registry     # 39 unit tests + 33 integration
cargo clippy -p erc7730 --all-targets --features uniffi,github-registry -- -D warnings
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
- `thiserror` for error types, `serde` for serialization
- No `.unwrap()` in library code — use `Result` and `?`
- All public API re-exported from `lib.rs`
- Signature-based decoding: function signatures parsed from descriptor format keys, no ABI JSON needed

## Spec Safety

Files implementing ERC-7730 spec behavior (`engine.rs`, `eip712.rs`, `decoder.rs`, `types/display.rs`, `types/context.rs`, `types/metadata.rs`) are guarded by 23 spec compliance tests + 33 integration tests.

Rules when editing these files:
1. Run `cargo test` after every edit to a spec-critical file — full suite takes <1s.
2. Do not change behavior adjacent to your task. If a refactor touches formatting logic, path resolution, or field rendering beyond the task scope — confirm with the user first.
3. If making a test pass requires changing behavior other tests depend on, explain the tradeoff BEFORE implementing. Do not modify spec compliance tests without explicit approval.
4. For ambiguous spec behavior, reference https://eips.ethereum.org/EIPS/eip-7730 — flag ambiguity rather than guessing.

## Public API

Shared types in `lib.rs`:
- `TransactionContext { chain_id, to, calldata, value, from }` — transaction parameters bundled into a single struct

Entry points in `lib.rs`:
- `format_calldata(descriptors, tx, data_provider)` — format calldata with pre-resolved descriptors; outer descriptor matched by chain_id + tx.to; remaining descriptors for nested calldata (Safe/4337); single-element slice = simple case
- `format_typed_data(descriptors, data, data_provider)` — format EIP-712 typed data with pre-resolved descriptors; outer descriptor matched by chain_id + verifying_contract

UniFFI FFI exports in `src/uniffi_compat/mod.rs`:
- `erc7730_format(chain_id, to, calldata_hex, value_hex, from_address, implementation_address, tokens)` — high-level with GitHub registry resolution (requires `github-registry` feature)
- `erc7730_format_typed(typed_data_json, tokens)` — high-level EIP-712 with GitHub registry resolution (requires `github-registry` feature)
- `erc7730_format_calldata(descriptors_json, chain_id, to, calldata_hex, value_hex, from_address, tokens)` — low-level calldata formatting
- `erc7730_format_typed_data(descriptors_json, typed_data_json, tokens)` — low-level EIP-712 formatting

Local Swift package product:
- `Erc7730` (binary target + Swift wrapper target)

## Key Modules

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `engine.rs` | `DisplayModel`, `DisplayEntry` (Item/Group/Nested), `DisplayItem` | Main formatting pipeline + nested calldata |
| `decoder.rs` | `FunctionSignature`, `ParamType`, `ArgumentValue` | Calldata decoding from function signatures |
| `eip712.rs` | `TypedData`, `TypedDataDomain` | EIP-712 typed data support |
| `resolver.rs` | `DescriptorSource` (trait), `ResolvedDescriptor`, `StaticSource`, `GitHubRegistrySource` | Descriptor resolution (static, HTTP) |
| `token.rs` | `TokenSource` (trait), `TokenMeta`, `WellKnownTokenSource`, `CompositeTokenSource` | Token metadata (CAIP-19 keys, embedded well-known tokens) |
| `address_book.rs` | `AddressBook` | Address → label resolution from descriptor metadata |
| `uniffi_compat/` | `TokenMetaInput`, `FfiError`, exported FFI functions | Stateless UniFFI wrapper layer |
| `types/` | `Descriptor`, `DescriptorContext`, `DescriptorDisplay`, `DisplayField`, `FieldFormat`, `VisibleRule` | Descriptor, display, context, metadata types |
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
- **`FieldFormat::Calldata`**: Nested calldata decoding (Safe `execTransaction`, ERC-4337 UserOps) — recursive rendering with `DisplayEntry::Nested`, `calleePath`/`amountPath`/`spenderPath` params, depth limit of 3
- **Batch operations (`wallet_sendCalls`)**: Handled wallet-side per spec — wallet calls `format_calldata()` per inner call, joins `interpolatedIntent` strings with " and ". No batch splitting in the engine.
- **`@.` container value priority**: Paths with `@.` prefix prefer container values over same-named function params (search from end)
- **Duplicate selector rejection**: Wallets MUST reject descriptors with multiple keys sharing the same selector (spec normative MUST)
- **Signed integer handling**: `int` types use two's complement → `BigInt` for correct negative display
- **`value` field on DisplayField**: Literal constant values as alternative to `path`
- **`separator` field**: Custom separator for array-typed values
- **`interoperableAddressName` format**: ERC-7930 stub with fallback to `addressName`
- **`date` encoding**: `"blockheight"` encoding shows block number instead of timestamp
- **`selectorPath`/`chainIdPath`**: Cross-field selector and chain ID resolution for nested calldata
- **`domainSeparator`**: EIP-712 context field (parsing only, validation is wallet-side)
- **Factory context**: `factory` object with `deployEvent` and `deployments`
- **EIP-712 format parity**: All 14 format types (including Duration, Unit, Amount, NftName, Raw) now work in EIP-712
- **EIP-712 AddressName**: Full senderAddress, sources, local/ENS resolution (parity with calldata)
- **Array slice syntax**: `[start:end]` in both calldata paths and EIP-712 paths
- **Unit SI prefix**: `prefix: true` enables k/M/G/T notation
- **Maps `keyPath`**: Cross-field key resolution for map lookups
- **`excluded` paths**: Deprecated v1 field now functional in rendering
- **Intent as object**: `intent` can be string or `{"label": "..."}` object
- **Interpolation escape sequences**: `{{` and `}}` produce literal braces
- **Encryption params**: `scheme` and `plaintextType` fields (parsing only)
- **EIP-712 domain completeness**: `version`, `chainId`, `salt` fields on descriptor domain

Optional features:
- `github-registry`: async HTTP descriptor fetching via `GitHubRegistrySource` (adds `reqwest` dependency; requires tokio runtime)
  - `GitHubRegistrySource::from_registry(base_url)` fetches `index.json` mapping `{chain_id}:{address}` → relative file path
  - Default registry: `https://github.com/llbartekll/7730-v2-registry` (v2 descriptors, index.json at root)
  - Registry source is cached via `tokio::sync::OnceCell` in FFI layer — index fetched once per process
  - UniFFI async exports use `#[uniffi::export(async_runtime = "tokio")]`; `uniffi` dep requires `features = ["tokio"]`

## Skills

- **`check-descriptor`** (`.claude/skills/check-descriptor/`): Validates ERC-7730 descriptor function signatures against on-chain contract ABIs via Etherscan. Trigger with `/check-descriptor <path-or-url>` or phrases like "check this descriptor", "validate descriptor against on-chain". Handles proxy contracts automatically.

## Environment

- **`ETHERSCAN_API_KEY`**: Available in `.env` at repo root. Load with `[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null)` before calling Etherscan. Use the V2 API: `https://api.etherscan.io/v2/api?chainid={id}&...`

## Pending

- **Phase 3**: `EmbeddedSource` + descriptor validation
- **Phase 4**: Packaging/distribution for existing UniFFI bindings (Swift XCFramework/SPM + Kotlin AAR/Maven)
- **Phase 5**: File inclusion (`$id`/includes), CI pipeline
