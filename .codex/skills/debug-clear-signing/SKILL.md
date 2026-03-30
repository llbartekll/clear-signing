---
name: debug-clear-signing
description: >
  Diagnose a wallet clear-signing failure from a pasted diagnostic capture or
  capture JSON file. Use when asked to "debug clear signing", "diagnose
  calldata capture", "diagnose typed data capture", "reproduce a clear-signing
  failure", or similar for either calldata or EIP-712.
---

# Debug Clear Signing

Diagnose wallet clear-signing failures from exported diagnostic captures and reproduce them locally.

## Use This Skill For

- Calldata clear-signing failures from the wallet diagnostic UI
- EIP-712 clear-signing failures from the wallet diagnostic UI
- Reproducing a failure offline from embedded descriptor JSON
- Narrowing the issue to wallet parsing, descriptor resolution/selection, descriptor content, or library formatting

## Inputs

- A pasted diagnostic JSON capture, or a path to a saved capture file
- Optional: permission to use network if the capture does not include embedded descriptors and live registry resolution is required

## Workflow

### 1. Validate the capture

Detect the capture type from the JSON shape:

- calldata: `calldata`, `to`, `chainId`
- typed data: `typedDataJson`, `summary`, `method`

Extract:

- `failedStage`
- `errorDescription` or legacy error field
- `resolvedDescriptorsJson`
- `selector` for calldata or `primaryType` for typed data
- proxy/matching fields such as `selectedDescriptorAddress`, `matchedAddress`, and `implementationAddress`

If the payload is malformed or missing the fields needed for classification, say so explicitly before attempting repro.

### 2. Run the repro helper

Use:

```bash
python3 .codex/skills/debug-clear-signing/scripts/replay_capture.py <capture-path>
```

If the user pasted JSON inline, save it to a temporary file under `/tmp` first, then run the helper on that file.

The helper:

- summarizes the capture
- generates a temporary Rust integration test
- replays the request through the Rust library
- prints `REPRO_STATUS=success` or `REPRO_STATUS=error`
- cleans up temporary artifacts unless `--keep-artifacts` is passed

If embedded descriptors are missing, the helper falls back to live registry resolution and uses `--features github-registry`. If sandboxed network access blocks that path, request escalation and continue.

### 3. Classify the issue

After the helper runs, classify the failure into one of:

- wallet/request parsing issue
- registry/descriptor resolution issue
- descriptor selection issue
- descriptor content issue
- library formatting bug or limitation
- signer-side issue

Do not stop at reporting the raw error string. Explain why the repro points to that category.

### 4. Report in a fixed shape

Always return:

- `failure stage`
- `reproduction result`
- `likely root cause`
- `why`
- `files to inspect`
- `minimal test to add`
- `next action`

### 5. Guardrails

- Prefer the embedded `resolvedDescriptorsJson` over live registry fetches
- Keep the repro offline-first
- Do not patch product code unless the user explicitly asks
- Do not modify spec-compliance tests unless the user explicitly asks
- If a field remains unresolved only because the repro uses `EmptyDataProvider`, call that out instead of misclassifying it as a descriptor bug

## References

- Read [REFERENCE.md](./REFERENCE.md) for capture schema notes, root-cause heuristics, and helper behavior
