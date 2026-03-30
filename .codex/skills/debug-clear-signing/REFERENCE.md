# Debug Clear Signing Reference

## Capture fields

### Calldata captures

Expected fields:

- `chainId`
- `to`
- `calldata`
- `selector`
- `failedStage`
- `resolvedDescriptorsJson`
- `selectedDescriptorAddress`
- `implementationAddress`
- `matchedAddress`
- `errorDescription`

### Typed-data captures

Expected fields:

- `chainId`
- `typedDataJson`
- `summary.primaryType`
- `summary.verifyingContract`
- `failedStage`
- `resolvedDescriptorsJson`
- `errorDescription`

## Failure-stage heuristics

### `failedStage = resolve`

Default suspicion:

- missing descriptor
- wrong target/verifying contract
- proxy mismatch
- typed-data domain mismatch
- registry/index issue

First files to inspect:

- `wallet/Wallet/Services/ClearSigningService.swift`
- `crates/clear-signing/src/resolver/nested_resolution.rs`
- `crates/clear-signing/src/resolver/typed_selection.rs`

### `failedStage = format`

Default suspicion:

- descriptor field/path bug
- unsupported value shape
- calldata decode mismatch
- EIP-712 render/domain validation issue
- engine formatting limitation

First files to inspect:

- calldata: `crates/clear-signing/src/engine.rs`, `crates/clear-signing/src/decoder.rs`
- typed data: `crates/clear-signing/src/eip712.rs`, `crates/clear-signing/src/eip712_domain.rs`

## Helper behavior

`scripts/replay_capture.py`:

- reads a capture JSON file
- detects calldata vs typed data
- prints a concise summary
- generates a temporary integration test under `crates/clear-signing/tests/`
- replays through the Rust library using embedded descriptors when available
- falls back to `GitHubRegistrySource::from_registry(...)` only when embedded descriptors are absent
- prints raw model data or raw error text
- deletes the temporary test file and temp capture copy unless `--keep-artifacts` is used

## Reporting rubric

After replay, decide whether the evidence points to:

- wallet/request parsing
- descriptor resolution or selection
- descriptor content
- data-provider gap
- library formatting bug or limitation

A good final answer should name the category, cite the likely code path, and propose the smallest regression test that would lock the behavior down.
