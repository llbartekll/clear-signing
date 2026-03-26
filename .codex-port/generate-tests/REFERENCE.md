# Generate Tests Reference

## Etherscan V2

Base endpoint:

```text
https://api.etherscan.io/v2/api
```

Transaction list:

```text
?chainid={chainId}&module=account&action=txlist&address={address}&startblock=0&endblock=99999999&page=1&offset=100&sort=desc&apikey={key}
```

Full transaction:

```text
?chainid={chainId}&module=proxy&action=eth_getTransactionByHash&txhash={hash}&apikey={key}
```

ABI fetch:

```text
?chainid={chainId}&module=contract&action=getabi&address={address}&apikey={key}
```

Unsupported or premium-gated chains should be skipped, not treated as fatal.

## Supported Chains In Practice

- `1` Ethereum Mainnet
- `10` Optimism
- `137` Polygon
- `42161` Arbitrum One
- `43114` Avalanche C-Chain

Other deployments may fail due to Etherscan plan limitations or missing support.

## ETHERSCAN_API_KEY

Load with:

```bash
[ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null); printenv ETHERSCAN_API_KEY
```

## Token Metadata

Resolve in this order:

1. `crates/erc7730/src/assets/tokens.json`
2. Alchemy `alchemy_getTokenMetadata`
3. Unknown token reported back to the user

Load Alchemy key with:

```bash
grep ALCHEMY_API_KEY wallet/Config.xcconfig | cut -d= -f2 | tr -d ' '
```

Alchemy subdomains:

- `1` -> `eth-mainnet`
- `10` -> `opt-mainnet`
- `137` -> `polygon-mainnet`
- `8453` -> `base-mainnet`
- `42161` -> `arb-mainnet`
- `43114` -> `avax-mainnet`

## Transaction Selection Heuristics

- Keep only successful transactions
- Match descriptor selectors on the first 4 bytes of calldata
- Prefer diversity in `from` and `value`
- Keep at most 3 examples per selector per deployment
- Report unmatched selectors as coverage gaps

## Permanent Test Pattern

For durable tests, follow the existing integration test style in `crates/erc7730/tests/`:

- load fixture JSON
- wrap into `ResolvedDescriptor`
- decode calldata
- build `TransactionContext`
- call `format_calldata(...)`
- assert on `intent` and specific rendered field values

Avoid snapshot-style assertions when a few explicit field checks are enough.
