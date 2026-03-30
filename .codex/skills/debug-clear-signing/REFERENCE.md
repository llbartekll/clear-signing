# Debug Clear Signing Reference

## Local registry

The first registry source of truth for this repo is the local fork next to the repo:

```text
../clear-signing-erc7730-registry--fork
```

Relevant files:

- `../clear-signing-erc7730-registry--fork/index.calldata.json`
- `../clear-signing-erc7730-registry--fork/index.eip712.json`

Check these before claiming a descriptor is missing.

Normalized index keys:

- calldata: `eip155:{chainId}:{address_lower}`
- typed data: `eip155:{chainId}:{verifying_contract_lower}`

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

Mandatory question before classifying as "missing descriptor":

- does the local registry fork contain the exact normalized key for this chain and address?

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

When reporting descriptor absence, say one of:

- `registry check: key present in local fork`
- `registry check: key absent in local fork`
- `registry check: not verified`

The third form should be treated as incomplete work, not a final conclusion.

A good final answer should name the category, cite the likely code path, and propose the smallest regression test that would lock the behavior down.
