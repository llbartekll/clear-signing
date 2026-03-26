---
name: generate-tests
description: >
  Fetch real blockchain transactions from Etherscan and validate them against an
  ERC-7730 descriptor using the Rust library. Reports pass/fail per function.
  Use when asked to "generate tests for this descriptor", "fetch real transactions
  for testing", "test descriptor with real data", or "/generate-tests <path-or-url>".
tools: Read, Bash, WebFetch, Grep, Glob
---

# generate-tests Skill

Validate an ERC-7730 descriptor end-to-end by fetching real on-chain transactions
and running them through the Rust clear-signing library. Reports pass/fail status
per function signature.

## Goal

For each function signature in a descriptor's `display.formats`, fetch real
transactions from Etherscan, run them through `format_calldata()`, and report
whether the library correctly formats each one.

This is a **smoke test tool** for descriptor authors — it answers "does my
descriptor work against real on-chain data?" The results are transient (not
persisted as fixtures). If you want permanent regression coverage, promote
interesting test cases to explicit integration tests in `crates/erc7730/tests/`.

## Inputs

- **Descriptor**: local file path OR GitHub URL
- **API key**: `ETHERSCAN_API_KEY` from env or `.env` file

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
   improvement suggestions (Step 7).

Build a token list for the validation run:
```json
[{"chain_id": 1, "address": "0x...", "symbol": "USDC", "decimals": 6, "name": "USD Coin"}]
```

### Step 5 — Run through library (validate)

For each matched transaction, fetch full calldata via Etherscan proxy API:
```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=proxy&action=eth_getTransactionByHash&txhash={hash}&apikey={key}
```
Or use `cast tx {hash} --rpc-url {rpc} input` if available.

Write a temporary Rust test file using the existing integration test pattern
(see `tests/aave_integration.rs`). The test must print full `DisplayModel`
output via `eprintln!` so it is captured in the `--nocapture` output.

**Test function template** (one per matched transaction):
```rust
#[tokio::test]
async fn smoke_{contract}_{function}() {
    let descriptor = load_descriptor("{descriptor_fixture}.json");
    let descriptors = wrap_rd(descriptor, {chain_id}, "{address}");
    let calldata = decode_hex("{calldata_hex}");
    let val = value_bytes("{value_hex}");
    let provider = /* token source with relevant tokens */;
    let tx = TransactionContext {
        chain_id: {chain_id}, to: "{address}",
        calldata: &calldata, value: val.as_deref(),
        from: Some("{from}"), implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("PASS {contract}/{function}");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [field] {}: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, entries, .. } => {
                        eprintln!("  [group] {label}:");
                        for sub in entries {
                            eprintln!("    {:?}", sub);
                        }
                    }
                    DisplayEntry::Nested { label, intent, entries, warnings } => {
                        eprintln!("  [nested] {label} (intent: {intent:?}):");
                        for sub in entries {
                            eprintln!("    {:?}", sub);
                        }
                        if !warnings.is_empty() {
                            eprintln!("    nested warnings: {warnings:?}");
                        }
                    }
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => eprintln!("FAIL {contract}/{function}: {e}"),
    }
    assert!(result.is_ok(), "{contract}/{function} failed: {:?}", result.err());
}
```

**Important**: Do NOT truncate or abbreviate the output. Print every field label
and value in full so the user can inspect the complete formatted result.

Run via:
```bash
cargo test -p erc7730 --test {temp_test_name} -- --nocapture 2>&1
```

Capture the full stderr output (where `eprintln!` goes). Parse it to extract
the structured results for the report.

Delete the temporary test file after collecting results.

### Step 6 — Report results

Present a structured report with **full field-level detail** for every test case.
Do NOT truncate field values or addresses — show every character.

```markdown
## Descriptor: {filename} — Real Transaction Validation

### Transactions Fetched
- Chain {id}: {total} txs → {matched} matched descriptor selectors

### Results

For each tested function, show:
1. Status (PASS/FAIL)
2. Intent string
3. Every field label and its full formatted value
4. Any warnings

Example:

#### 1. supply(...) — PASS
- **Tx**: `0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef`
- **Intent**: "Supply"
- **Owner**: "Protocol Name"
- Fields:
  - **Asset**: `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48`
  - **Amount**: `1000.50 USDC`
  - **On Behalf Of**: `0x1234567890AbCdEf1234567890aBcDeF12345678`

#### 2. withdraw(...) — FAIL
- **Tx**: `0xabcdef...`
- **Error**: `no matching format key for selector 0x12345678`

### Coverage
- {matched}/{total} format keys have matching on-chain transactions
- {uncovered function names} — no matching txs in last 100

### Uncovered Selectors (not in descriptor)
- 0x28530a47 — 3 txs (use `cast 4byte 0x28530a47` to identify)

### Analysis

After presenting results, analyze the formatting output for issues:
- **Intent consistency**: Are intent strings consistently cased? (e.g., "claim withdrawal" vs "Claim Withdrawal")
- **Token resolution**: Were all token amounts formatted with symbols and decimals, or are any showing raw wei values?
- **Address display**: Are addresses showing checksummed (EIP-55) format?
- **Missing labels**: Are any fields showing generic labels like "Param 0" instead of descriptive names?
- **Warnings**: Flag any warnings emitted by the library (e.g., "token metadata not found")
- **Value formatting**: Are large numbers human-readable? Are decimals reasonable?
- **Nested calldata**: If any functions have nested calls, are they rendered with proper inner intent?

Present issues as a checklist:
- [ ] Issue description — which function, which field, what's wrong
- [ ] ...

If no issues found, state: "No formatting issues detected."
```

#### Optional: Scaffold integration test

If the user asks, print a scaffold of an integration test to stdout (following
the pattern in `tests/morpho_blue_integration.rs` or `tests/aave_integration.rs`)
that they can copy-paste and customize with explicit assertions. This is a
convenience feature — the skill does not write test files.

### Step 7 — Self-diagnostics + improvement suggestions

After the run completes, review all issues encountered during execution and suggest
concrete, actionable fixes:

- API call returned unexpected format → suggest updating REFERENCE.md with correct schema
- Chain returned error / not supported → suggest adding to premium-only list in chain table
- Token resolution failed → suggest adding token to `crates/erc7730/src/assets/tokens.json`
- Alchemy subdomain returned error → suggest updating subdomain mapping
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

## Scope

- **Calldata only** — EIP-712 typed data signatures are off-chain and not fetchable from Etherscan
- **Successful transactions only** — reverted txs are skipped
