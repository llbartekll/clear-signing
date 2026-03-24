---
name: generate-tests
description: >
  Fetch real blockchain transactions from Etherscan, validate them against an
  ERC-7730 descriptor using the Rust library, and persist regression fixture tests.
  Use when asked to "generate tests for this descriptor", "fetch real transactions
  for testing", "test descriptor with real data", or "/generate-tests <path-or-url>".
tools: Read, Bash, WebFetch, Grep, Glob, Write
---

# generate-tests Skill

Validate an ERC-7730 descriptor end-to-end by fetching real on-chain transactions,
running them through the Rust clear-signing library, and persisting regression
fixture tests.

## Goal

For each function signature in a descriptor's `display.formats`, fetch real
transactions from Etherscan, run them through `format_calldata()`, and persist
1 calldata per function as a regression fixture test. The fixture is auto-discovered
by `fixture_tests.rs` and runs on every `cargo test`.

**Single flow — every run:**
1. Validates the descriptor against real transactions (transient report)
2. Writes `tests/fixtures/{name}/tests.json` with 1 tx per function (persistent regression test)
3. Runs `cargo test` to populate expected values via capture mode

**Re-run policy:** Overwrites existing fixtures. Re-running means intentionally
resetting the regression baseline (descriptor or engine changed). Day-to-day
`cargo test` uses committed expected values to catch regressions.

## Inputs

- **Descriptor**: local file path OR GitHub URL
- **API key**: `ETHERSCAN_API_KEY` from env or `.env` file
- If no descriptor specified: process all entries from contract registry (`.test-contracts.json`)

## Workflow

### Step 0 — Load API keys

```bash
[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null); printenv ETHERSCAN_API_KEY
```

If empty → abort with: "Set `ETHERSCAN_API_KEY` in `.env` or environment to fetch transactions."

Also load Alchemy key for token resolution:
```bash
grep ALCHEMY_API_KEY wallet/Config.xcconfig | cut -d= -f2 | tr -d ' '
```

If empty → warn: "No Alchemy key found. Token resolution limited to well-known tokens."

### Step 1 — Obtain descriptor JSON

If input looks like a URL:
- Convert GitHub blob URLs to raw: `github.com/{owner}/{repo}/blob/{branch}/path` → `raw.githubusercontent.com/{owner}/{repo}/{branch}/path`
- Fetch with WebFetch

If input is a local file path:
- Read with Read tool

If no input specified:
- Read `crates/erc7730/tests/.test-contracts.json` (contract registry)
- For each entry, use `local_fixture` to find the descriptor in `tests/fixtures/`
- Process each descriptor in sequence

Parse JSON into memory. Extract:
```
deployments = descriptor.context.contract.deployments  # [{chainId, address}, ...]
format_keys = Object.keys(descriptor.display.formats)   # function signature strings
```

Derive the fixture name from the descriptor filename (strip `.json`). Ensure the
descriptor JSON exists at `crates/erc7730/tests/fixtures/{name}.json` — copy it there
if not already present.

### Step 2 — Compute selectors

For each format key, compute the 4-byte selector. Prefer `cast` (Foundry) if available:

```bash
which cast && cast sig "supply(address,uint256,address,uint16)"
```

Fallback: use the Python keccak snippet from check-descriptor's REFERENCE.md.

Build a mapping: `selector → format_key` (e.g., `0x617ba037 → supply(address,uint256,address,uint16)`).

### Step 3 — Fetch real transactions

For each deployment `(chainId, address)`:

```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=account&action=txlist&address={address}&startblock=0&endblock=99999999&page=1&offset=100&sort=desc&apikey={key}
```

**Chain error handling:** If the response `status != "1"` or the message contains
"premium", "paid", or indicates the chain is unsupported:
- Log: `[WARN] Chain {chainId}: skipped — {reason}`
- Continue to the next deployment. Do NOT abort the whole run.

From the response `result` array, filter transactions:
- `tx.input` starts with a known selector (first 10 chars = `0x` + 8 hex matching a format key)
- `tx.isError == "0"` (successful transactions only)
- `tx.to` matches the deployment address (case-insensitive)

Group by selector. Take up to **3 transactions per selector** for validation,
preferring diverse `from` addresses and `value` amounts.

Also track transactions whose selectors do NOT match any format key — these represent
**uncovered functions** (like L2 `withdraw(bytes32)` variants).

### Step 4 — Resolve token metadata

For each format key that has `tokenAmount` fields with a `tokenPath`, identify the
token address from fetched calldata at the referenced parameter position.

**Resolution order:**

1. **Well-known tokens** (no network): Look up in `crates/erc7730/src/assets/tokens.json`
   using CAIP-19 key `eip155:{chainId}/erc20:{addr_lowercase}`.

2. **Alchemy API** (fallback): Call `alchemy_getTokenMetadata` per REFERENCE.md.
   Use the chain-specific subdomain from the Alchemy chain table.

3. **Unknown**: If both fail, note the token address as unresolved. Add to the
   improvement suggestions (Step 8).

Build a `tokens` array for the fixture:
```json
[{"chain_id": 1, "address": "0x...", "symbol": "USDC", "decimals": 6, "name": "USD Coin"}]
```

### Step 5 — Run through library + write fixtures

#### 5a. Validate (transient)

For each matched transaction, fetch full calldata via Etherscan proxy API:
```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=proxy&action=eth_getTransactionByHash&txhash={hash}&apikey={key}
```
Or use `cast tx {hash} --rpc-url {rpc} input` if available.

Write a temporary Rust test file using the existing integration test pattern
(see `tests/aave_integration.rs`):
```rust
let tx = TransactionContext { chain_id, to, calldata: &calldata, value, from, implementation_address: None };
let descriptors = vec![ResolvedDescriptor { descriptor, chain_id, address: to.to_lowercase() }];
let result = format_calldata(&descriptors, &tx, &provider).await;
```

Run via:
```bash
cargo test -p erc7730 --test {temp_test_name} -- --nocapture
```

Capture whether each call succeeds or fails. If it succeeds, capture the
`DisplayModel` output (intent, interpolated intent, entries, warnings).

Delete the temporary test file after collecting results.

#### 5b. Write regression fixture (persistent)

1. Pick exactly **1 successful transaction per function selector** (prefer mainnet chain,
   first successful match across chains).
2. Create directory `crates/erc7730/tests/fixtures/{name}/` (overwrite if exists).
3. Write `tests.json` with:
   - `"descriptor": "../{name}.json"` (relative path to descriptor)
   - `"tokens": [...]` from Step 4 resolution
   - `"tests": [...]` with one entry per function, `"expected": null` (capture mode)
   - Test `"name"` derived from function name in snake_case (strip parens and params,
     e.g., `swapExactAmountIn(...)` → `swap_exact_amount_in`)
4. Run `cargo test -p erc7730 --test fixture_tests -- --nocapture` to trigger capture mode.
   This populates the `expected` fields in `tests.json` with actual `DisplayModel` output.
5. The resulting `tests.json` with populated expected values is ready to commit.

### Step 6 — Report results

Present a structured report:

```markdown
## Descriptor: {filename} — Real Transaction Validation

### Transactions Fetched
- Chain {id}: {total} txs → {matched} matched descriptor selectors

### Results

| # | Function | Chain | Tx | Status | Intent |
|---|----------|-------|----|--------|--------|
| 1 | supply(...) | 1 | 0x1234...abcd | PASS | Supply |
| 2 | withdraw(...) | 1 | 0x5678...ef01 | PASS | Withdraw |
| 3 | repay(...) | 10 | 0x9abc...2345 | FAIL: missing token | Repay loan |

### Coverage
- 7/8 format keys have matching on-chain transactions
- repayWithPermit — no matching txs in last 100

### Uncovered Selectors (not in descriptor)
- 0x28530a47 setUserEMode(uint8) — 3 txs
- 0x2dad97d4 repayWithATokens(address,uint256,uint256) — 3 txs

### Fixtures: crates/erc7730/tests/fixtures/{name}/tests.json
- {N} regression test cases (1 per function)
- Expected values captured via cargo test
- Tokens resolved: {list}
- Missing tokens: {list or "none"}
```

### Step 7 — Update contract registry

If `crates/erc7730/tests/.test-contracts.json` exists:
- Find or create the entry for this descriptor
- Update `last_validated` to today's date
- Update `chains` with the chains that were successfully queried

If it does not exist, create it with the first entry:
```json
[
  {
    "name": "{name}",
    "descriptor_url": "{url_if_provided}",
    "local_fixture": "{name}.json",
    "chains": [1],
    "last_validated": "2026-03-24"
  }
]
```

### Step 8 — Self-diagnostics + improvement suggestions

After the run completes, review all issues encountered during execution and suggest
concrete, actionable fixes to the skill itself or the project:

- API call returned unexpected format → suggest updating REFERENCE.md with correct schema
- Chain returned error / not supported → suggest adding to premium-only list in chain table
- Token resolution failed → suggest adding token to `crates/erc7730/src/assets/tokens.json`
- Alchemy subdomain returned error → suggest updating subdomain mapping
- Fixture `tests.json` schema didn't match `fixture_tests.rs` expectations → suggest schema fix
- Etherscan rate-limited → suggest reducing batch size or adding delay between calls
- Selector computation failed → suggest updating fallback method
- New uncovered patterns (e.g., new `DisplayField` variant needed) → suggest engine improvement
- Descriptor parsing failed → suggest investigating missing spec features

Format as an actionable checklist at the end of the report:

```markdown
## Skill Improvement Suggestions
- [ ] Add chain 56 (BNB) to premium-only list in REFERENCE.md — txlist returned "premium required"
- [ ] Add PEPE (0x6982...) on chain 1 to tokens.json — Alchemy resolved: decimals=18, symbol=PEPE
- [ ] Update Alchemy subdomain for Avalanche — current `avax-mainnet` returned 404
```

If no issues were encountered, omit this section.

## Error Handling

- **No Etherscan API key**: abort early with clear message
- **No Alchemy key**: warn, fall back to well-known tokens only
- **Premium-only chain**: log warning, skip chain, continue with others
- **Etherscan rate limit (429)**: note it, continue with other chains
- **No transactions found**: report per-chain, still show coverage gaps
- **Library formatting error**: report as FAIL with error message, continue
- **Missing token metadata**: report as warning, include in improvement suggestions
- **Overwrite existing fixtures**: no confirmation needed, just overwrite and note in report

## Scope

- **Calldata only** — EIP-712 typed data signatures are off-chain and not fetchable from Etherscan
- **Successful transactions only** — reverted txs are skipped
