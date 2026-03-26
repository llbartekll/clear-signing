---
name: generate-tests
description: >
  Fetch real blockchain transactions from Etherscan and validate them against an
  ERC-7730 descriptor using the Rust library. Use when asked to "generate tests
  for this descriptor", "fetch real transactions for testing", "test descriptor
  with real data", or "smoke test this descriptor against mainnet activity".
---

# Generate Tests

Validate an ERC-7730 descriptor end-to-end against real on-chain transactions.

## Use This Skill For

- Descriptor smoke-testing against production calldata
- Generating candidate integration tests from real transactions
- Identifying selector coverage gaps between a descriptor and deployed usage

## Inputs

- Descriptor path in the workspace, or a raw/GitHub URL
- `ETHERSCAN_API_KEY` from env or `.env`

If the task requires network access and the sandbox blocks `curl`, request escalation and continue.

## Workflow

### 1. Load keys and repo context

Run:

```bash
[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null); printenv ETHERSCAN_API_KEY
grep ALCHEMY_API_KEY wallet/Config.xcconfig | cut -d= -f2 | tr -d ' '
```

If `ETHERSCAN_API_KEY` is empty, stop and tell the user that on-chain fetches cannot proceed.

If the Alchemy key is empty, continue with a warning that token resolution is limited to embedded well-known tokens.

### 2. Obtain and inspect the descriptor

For local descriptors, read the file directly.

For GitHub URLs:
- Convert `github.com/.../blob/...` URLs to `raw.githubusercontent.com/...`
- Fetch with `curl`

Extract:

```text
deployments = descriptor.context.contract.deployments
format_keys = descriptor.display.formats keys
```

Derive a fixture name from the descriptor filename and ensure a copy exists in:

```text
crates/erc7730/tests/fixtures/{name}.json
```

Use the existing fixture if one already matches. Do not overwrite unrelated user changes.

### 3. Compute selectors

Prefer Foundry:

```bash
which cast && cast sig "transfer(address,uint256)"
```

Fallback to the selector logic already documented in `.claude/skills/check-descriptor/REFERENCE.md`.

Build `selector -> format_key` so fetched transactions can be matched back to descriptor entries.

### 4. Fetch candidate transactions

For each deployment `(chainId, address)`, call Etherscan V2:

```text
https://api.etherscan.io/v2/api?chainid={chainId}&module=account&action=txlist&address={address}&startblock=0&endblock=99999999&page=1&offset=100&sort=desc&apikey={key}
```

Filter transactions to:

- `isError == "0"`
- `to` equals the deployment address, case-insensitive
- `input` starts with a selector known to the descriptor

Keep up to 3 transactions per selector, preferring different senders and value amounts when possible.

Track unknown selectors separately as uncovered on-chain functions.

If a chain is unsupported, premium-gated, or otherwise rejected by Etherscan, log it and continue with other deployments.

### 5. Resolve token metadata

For token-aware fields, resolve token addresses referenced by calldata and build the smallest possible token set for the validation run.

Resolution order:

1. `crates/erc7730/src/assets/tokens.json`
2. Alchemy `alchemy_getTokenMetadata`
3. Mark unresolved and report it

### 6. Validate through the Rust library

Fetch full calldata per selected transaction via Etherscan proxy API:

```text
https://api.etherscan.io/v2/api?chainid={chainId}&module=proxy&action=eth_getTransactionByHash&txhash={hash}&apikey={key}
```

Create a temporary integration test under `crates/erc7730/tests/` following the existing patterns in the repo. The test should:

- Load the descriptor fixture
- Wrap it into `ResolvedDescriptor`
- Build `TransactionContext`
- Call `format_calldata(...)`
- Print full `DisplayModel` details with `eprintln!`
- Assert `result.is_ok()`

Do not truncate rendered fields in the test output.

If you touch spec-critical files instead of only adding a temporary test, run the full `cargo test` suite immediately after each such edit.

Run only the temporary test first:

```bash
cargo test -p erc7730 --test {temp_test_name} -- --nocapture
```

Capture the printed output, then delete the temporary test file unless the user explicitly asked to keep generated tests.

### 7. Report results

Report:

- Deployments checked
- Transactions fetched and matched per chain
- PASS/FAIL per tested function
- Full intent/interpolated intent when available
- Full field label/value output for each successful formatting result
- Unknown selectors and other coverage gaps
- Unresolved tokens

If the user wants permanent coverage, promote the strongest cases into explicit integration tests in `crates/erc7730/tests/` with concrete assertions on intent and key rendered fields.

### 8. Diagnose whether the issue is in the engine

After every run, do a short root-cause pass on any failures, warnings, or `<unresolved>` fields.

Classify each issue as one of:

- Descriptor issue
- Data-provider or metadata issue
- Engine limitation or bug

Treat it as an engine issue when the descriptor intent and core decoding succeed but fields fail because the library cannot navigate or format the requested value shape. Common examples:

- path syntax accepted by the descriptor but unsupported in the current resolver
- byte or tuple slicing that the engine does not currently handle
- values that resolve, but cannot be coerced into the formatter's expected type

When you conclude it is likely an engine issue:

- say so explicitly in the final report
- cite the relevant Rust file and function
- explain the mismatch between descriptor expectation and current engine behavior
- distinguish "engine limitation" from "descriptor typo" if the path looks internally consistent

Do not stop at saying a field is `<unresolved>`; explain why it became unresolved when the code makes that diagnosable.

## Guardrails

- Prefer existing repo helpers and test patterns over inventing a new harness
- Do not modify spec compliance tests unless the user explicitly asks
- Do not refactor adjacent formatting logic just to support one generated test
- If network access is blocked, request escalation rather than pretending the skill can complete offline
- Clean up temporary test artifacts after collecting results, unless the user asked to keep them

## References

- Read [REFERENCE.md](./REFERENCE.md) for Etherscan endpoints, Alchemy chain mapping, transaction heuristics, and the integration-test pattern
