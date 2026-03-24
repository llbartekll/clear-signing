# generate-tests Reference

## Etherscan V2 API

Base endpoint: `https://api.etherscan.io/v2/api`

### Transaction List

```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=account&action=txlist&address={address}&startblock=0&endblock=99999999&page=1&offset=100&sort=desc&apikey={key}
```

Response shape:
```json
{
  "status": "1",
  "message": "OK",
  "result": [
    {
      "hash": "0xabc...",
      "from": "0x...",
      "to": "0x...",
      "value": "0",
      "input": "0x617ba037000000000000000000000000...",
      "isError": "0",
      "blockNumber": "12345678",
      "timeStamp": "1700000000"
    }
  ]
}
```

Key fields per transaction:
- `hash`: transaction hash
- `from`: sender address
- `to`: recipient (contract) address
- `value`: ETH value in wei (decimal string)
- `input`: calldata hex string (starts with `0x`)
- `isError`: `"0"` = success, `"1"` = reverted

### Full Calldata via Proxy API

The txlist endpoint may truncate `input`. Fetch full calldata per transaction:

```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=proxy&action=eth_getTransactionByHash&txhash={hash}&apikey={key}
```

Response: `result.input` contains full calldata.

Alternative: `cast tx {hash} --rpc-url {rpc} input` if Foundry is available.

### Contract ABI (reused from check-descriptor)

```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=contract&action=getabi&address={address}&apikey={key}
```

### Supported Chain IDs

| Chain | ID | Free Tier |
|-------|----|-----------|
| Ethereum Mainnet | 1 | Yes |
| Optimism | 10 | Yes |
| BNB Smart Chain | 56 | No (premium) |
| Polygon | 137 | Yes |
| Base | 8453 | No (premium) |
| Arbitrum One | 42161 | Yes |
| Avalanche C-Chain | 43114 | Yes |
| Celo | 42220 | No (unsupported) |
| Linea | 59144 | No (unsupported) |
| zkSync Era | 324 | No (unsupported) |
| Sepolia testnet | 11155111 | Unreliable |

When a chain returns an error or "premium required" message, skip it and continue with other chains.

---

## ETHERSCAN_API_KEY

Load from `.env`:
```bash
[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null); printenv ETHERSCAN_API_KEY
```

Free keys: https://etherscan.io/apis (V2 multi-chain API covers all supported chains).

---

## Token Metadata Resolution

Resolve token metadata in this order:

### 1. Well-known tokens (no network)

Look up in `crates/erc7730/src/assets/tokens.json` using CAIP-19 key format:
```
eip155:{chainId}/erc20:{address_lowercase}
```

Example: `eip155:1/erc20:0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48` → USDC.

### 2. Alchemy API (fallback)

Use `alchemy_getTokenMetadata` — single call returns symbol, decimals, and name.

```bash
curl -s -X POST "https://{subdomain}.g.alchemy.com/v2/{key}" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"alchemy_getTokenMetadata","params":["0xtoken_address"],"id":1}'
```

Response:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "symbol": "USDC",
    "decimals": 6,
    "name": "USD Coin",
    "logo": "https://..."
  }
}
```

### 3. Unknown

If both fail, note the token as unknown and suggest adding it to `tokens.json`.

---

## Alchemy API

### API Key

Load from `wallet/Config.xcconfig`:
```bash
grep ALCHEMY_API_KEY wallet/Config.xcconfig | cut -d= -f2 | tr -d ' '
```

### Chain Subdomains

| Chain | ID | Alchemy subdomain |
|-------|----|-------------------|
| Ethereum | 1 | `eth-mainnet` |
| Optimism | 10 | `opt-mainnet` |
| Polygon | 137 | `polygon-mainnet` |
| Polygon (RPC fallback) | 137 | `https://polygon-bor-rpc.publicnode.com` |
| Base | 8453 | `base-mainnet` |
| Arbitrum | 42161 | `arb-mainnet` |
| Avalanche | 43114 | `avax-mainnet` |

Endpoint pattern: `https://{subdomain}.g.alchemy.com/v2/{key}`

---

## Transaction Selection Heuristics

When selecting transactions for test cases:

1. **Filter**: `isError == "0"` and `to` matches deployment address (case-insensitive)
2. **Match**: first 10 chars of `input` (`0x` + 8 hex) equal a known selector
3. **Diversity**: prefer transactions with different `from` addresses and `value` amounts
4. **Limit**: up to 3 transactions per selector per chain for validation
5. **Fixture**: exactly 1 transaction per selector (prefer mainnet, first successful match)
6. **Uncovered**: track selectors NOT in the descriptor — report as coverage gaps

---

## Selector Computation

Same as check-descriptor. Prefer `cast sig "..."` if Foundry is installed.

Fallback: Python keccak snippet from `check-descriptor/REFERENCE.md`.

---

## Uncovered Selector Identification

Use `cast 4byte {selector}` to reverse-lookup unknown selectors via the 4byte directory.
This helps identify functions that exist on-chain but lack descriptor coverage.

---

## Value Conversion

Etherscan returns `value` as a decimal string (wei). Convert to hex for the library:

```python
value_wei = int(tx["value"])
value_hex = "0x0" if value_wei == 0 else hex(value_wei)
```

The test code converts hex values to the 32-byte big-endian format expected by
`format_calldata()`.

---

## GitHub URL Conversion

Same as check-descriptor:
```
github.com/{owner}/{repo}/blob/{branch}/path
→ raw.githubusercontent.com/{owner}/{repo}/{branch}/path
```

---

## Contract Registry

File: `crates/erc7730/tests/.test-contracts.json` (gitignored)

Tracks which descriptors have been validated and when. The skill reads this when invoked
without a specific descriptor, and updates `last_validated` after successful runs.

Format:
```json
[
  {
    "name": "aave-v3",
    "descriptor_url": "https://github.com/llbartekll/7730-v2-registry/blob/main/registry/aave/calldata-Pool-v3.json",
    "local_fixture": "aave-lpv3.json",
    "chains": [1, 10, 137, 42161, 43114, 8453],
    "last_validated": "2026-03-24"
  }
]
```

Fields:
- `name`: human-readable identifier
- `descriptor_url`: GitHub URL for the descriptor (used to re-fetch if needed)
- `local_fixture`: filename in `tests/fixtures/` (the descriptor JSON)
- `chains`: chain IDs to test against (free-tier only)
- `last_validated`: ISO date of last successful run

---

## Fixture Output Format

Generated fixture files are auto-discovered by `fixture_tests.rs` at
`crates/erc7730/tests/fixtures/*/tests.json`.

Directory structure:
```
crates/erc7730/tests/fixtures/
├── aave-lpv3.json              (descriptor JSON)
├── aave-lpv3/
│   └── tests.json              (generated regression fixture)
├── paraswap-v6.2.json
├── paraswap-v6.2/
│   └── tests.json
```

`tests.json` schema (matches `FixtureSuite` in `fixture_tests.rs`):
```json
{
  "descriptor": "../aave-lpv3.json",
  "tokens": [
    {
      "chain_id": 1,
      "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
      "symbol": "USDC",
      "decimals": 6,
      "name": "USD Coin"
    }
  ],
  "tests": [
    {
      "name": "supply",
      "tx_hash": "0xabc123...",
      "chain_id": 1,
      "to": "0x87870bca3f3fd6335c3f4ce8392d69350b4fa4e2",
      "calldata": "0x617ba037...",
      "value": "0x0",
      "from": "0xsender...",
      "expected": null
    }
  ]
}
```

- `expected: null` triggers capture mode — `fixture_tests.rs` runs the test, populates
  expected output, and rewrites `tests.json`
- `expected: { ... }` enables regression mode — actual output is compared against stored snapshot
- Test names are derived from function names in snake_case (e.g., `supply`, `swap_exact_amount_in`)
