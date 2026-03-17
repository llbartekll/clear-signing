---
name: generate-tests
description: >
  Fetch real blockchain transactions from Etherscan and validate them against the
  ERC-7730 descriptor using the Rust library. Use when asked to "generate tests for
  this descriptor", "fetch real transactions for testing", "test descriptor with
  real data", or "/generate-tests <path-or-url>".
tools: Read, Bash, WebFetch, Grep, Glob
---

# generate-tests Skill

Validate an ERC-7730 descriptor end-to-end by fetching real on-chain transactions
and running them through the Rust clear-signing library.

## Goal

For each function signature in a descriptor's `display.formats`, fetch real
transactions from Etherscan, run them through `format_calldata()`, and
report whether formatting succeeds with correct output. No files are written —
this is transient validation for quick feedback.

## Inputs

- **Descriptor**: local file path OR GitHub URL (same as check-descriptor)
- **API key**: `ETHERSCAN_API_KEY` from env or `.env` file

## Workflow

### Step 0 — Load API key

```bash
[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null); printenv ETHERSCAN_API_KEY
```

If empty → abort with: "Set `ETHERSCAN_API_KEY` in `.env` or environment to fetch transactions."

### Step 1 — Obtain descriptor JSON

If input looks like a URL:
- Convert GitHub blob URLs to raw: `github.com/{owner}/{repo}/blob/{branch}/path` → `raw.githubusercontent.com/{owner}/{repo}/{branch}/path`
- Fetch with WebFetch

If input is a local file path:
- Read with Read tool

Parse JSON into memory. Extract:
```
deployments = descriptor.context.contract.deployments  # [{chainId, address}, ...]
format_keys = Object.keys(descriptor.display.formats)   # function signature strings
```

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

From the response `result` array, filter transactions:
- `tx.input` starts with a known selector (first 10 chars = `0x` + 8 hex matching a format key)
- `tx.isError == "0"` (successful transactions only)
- `tx.to` matches the deployment address (case-insensitive)

Group by selector. Take up to **3 transactions per selector**, preferring diverse `from` addresses and `value` amounts.

Also track transactions whose selectors do NOT match any format key — these represent **uncovered functions** (like L2 `withdraw(bytes32)` variants).

### Step 4 — Identify token metadata

For each format key that has `tokenAmount` fields with a `tokenPath`, decode the
token address from the calldata at the referenced parameter position.

1. Check `WellKnownTokenSource` first (embedded `tokens.json` in the library)
2. If not found, fetch token info from Etherscan:
   ```
   GET https://api.etherscan.io/v2/api?chainid={chainId}&module=token&action=tokeninfo&contractaddress={tokenAddr}&apikey={key}
   ```
3. If a token is found on-chain but missing from `tokens.json`, report a suggestion:
   ```
   Suggestion: Add {symbol} ({address}) on chain {chainId} to crates/erc7730/src/tokens.json
   ```

Build a `tokens` array for the test cases:
```json
[{"chain_id": 1, "address": "0x...", "symbol": "USDC", "decimals": 6, "name": "USD Coin"}]
```

### Step 5 — Run through library

Write a temporary Rust test file to validate each transaction. The approach:

1. For each matched transaction, fetch full calldata via Etherscan proxy API:
   ```
   GET https://api.etherscan.io/v2/api?chainid={chainId}&module=proxy&action=eth_getTransactionByHash&txhash={hash}&apikey={key}
   ```
   Or use `cast tx {hash} --rpc-url {rpc} input` if available.

2. Write a temporary test file using the existing integration test pattern
   (see `tests/aave_integration.rs`):
   ```rust
   let tx = TransactionContext { chain_id, to, calldata: &calldata, value, from };
   let result = format_calldata(&descriptor, &tx, &tokens);
   ```

3. Run via:
   ```bash
   cargo test -p erc7730 --test {temp_test_name} -- --nocapture
   ```

4. Capture whether each call succeeds or fails, and if it succeeds, capture the
   `DisplayModel` output (intent, interpolated intent, entries, warnings).

5. Delete the temporary test file after collecting results.

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

### Token Suggestions
- Add WETH (0xC02a...6Cc2) on chain 1 to tokens.json
```

## Error Handling

- **No API key**: abort early with clear message
- **Etherscan rate limit (429)**: note it, continue with other chains
- **No transactions found**: report per-chain, still show coverage gaps
- **Library formatting error**: report as FAIL with error message, continue
- **Missing token metadata**: report as warning, suggest adding to tokens.json

## Scope

- **Calldata only** — EIP-712 typed data signatures are off-chain and not fetchable from Etherscan
- **Successful transactions only** — reverted txs are skipped
