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

When selecting transactions for validation:

1. **Filter**: `isError == "0"` and `to` matches deployment address (case-insensitive)
2. **Match**: first 10 chars of `input` (`0x` + 8 hex) equal a known selector
3. **Diversity**: prefer transactions with different `from` addresses and `value` amounts
4. **Limit**: up to 3 transactions per selector per chain for validation
5. **Uncovered**: track selectors NOT in the descriptor — report as coverage gaps

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

The integration test code converts hex values to the 32-byte big-endian format
expected by `format_calldata()`.

---

## GitHub URL Conversion

Same as check-descriptor:
```
github.com/{owner}/{repo}/blob/{branch}/path
→ raw.githubusercontent.com/{owner}/{repo}/{branch}/path
```

---

## Integration Test Pattern

When scaffolding integration tests from validated transactions, follow the pattern
established in `crates/erc7730/tests/morpho_blue_integration.rs`:

```rust
fn load_descriptor(fixture: &str) -> Descriptor { ... }
fn wrap_rd(descriptor, chain_id, address) -> Vec<ResolvedDescriptor> { ... }
fn decode_hex(hex_str: &str) -> Vec<u8> { ... }
fn get_entry_value(model: &DisplayModel, label: &str) -> String { ... }

#[tokio::test]
async fn test_function_name() {
    let descriptor = load_descriptor("descriptor-name.json");
    let descriptors = wrap_rd(descriptor, chain_id, "0xcontract");
    let calldata = decode_hex("0x...");
    let tx = TransactionContext { chain_id, to, calldata: &calldata, value: None, from: Some("0x..."), implementation_address: None };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();
    assert_eq!(result.intent, "Expected Intent");
    assert_eq!(get_entry_value(&result, "Label"), "expected value");
}
```

Key: use explicit assertions on intent and specific field values, not snapshot comparison.
