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

### Token Info

```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=token&action=tokeninfo&contractaddress={tokenAddr}&apikey={key}
```

Response shape:
```json
{
  "status": "1",
  "message": "OK",
  "result": [
    {
      "contractAddress": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
      "tokenName": "USD Coin",
      "symbol": "USDC",
      "divisor": "6",
      "tokenType": "ERC-20"
    }
  ]
}
```

`divisor` is the number of decimals (as a string).

Note: this endpoint requires an API Pro key. For free-tier keys, use well-known
token metadata or look up common tokens manually.

### Contract ABI (reused from check-descriptor)

```
GET https://api.etherscan.io/v2/api?chainid={chainId}&module=contract&action=getabi&address={address}&apikey={key}
```

### Supported Chain IDs

| Chain | ID |
|-------|----|
| Ethereum Mainnet | 1 |
| Optimism | 10 |
| BNB Smart Chain | 56 |
| Polygon | 137 |
| Base | 8453 |
| Arbitrum One | 42161 |
| Avalanche C-Chain | 43114 |
| Sepolia testnet | 11155111 |

---

## ETHERSCAN_API_KEY

Load from `.env`:
```bash
[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null); printenv ETHERSCAN_API_KEY
```

Free keys: https://etherscan.io/apis (V2 multi-chain API covers all supported chains).

---

## Transaction Selection Heuristics

When selecting transactions for test cases:

1. **Filter**: `isError == "0"` and `to` matches deployment address (case-insensitive)
2. **Match**: first 10 chars of `input` (`0x` + 8 hex) equal a known selector
3. **Diversity**: prefer transactions with different `from` addresses and `value` amounts
4. **Limit**: up to 3 transactions per selector per chain
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

The test code converts hex values to the 32-byte big-endian format expected by
`format_calldata()`.

---

## GitHub URL Conversion

Same as check-descriptor:
```
github.com/{owner}/{repo}/blob/{branch}/path
→ raw.githubusercontent.com/{owner}/{repo}/{branch}/path
```
