# clear-signing

ERC-7730 v2 clear-signing library for Rust. Decodes and formats Ethereum
contract calldata and EIP-712 typed data into human-readable display models
for wallets and signers.

## Install

```sh
cargo add clear-signing
```

Optional features:

- `uniffi` — UniFFI export layer for cross-language bindings (Swift, Kotlin)
- `github-registry` — async HTTP descriptor source backed by a GitHub-hosted registry

## Usage

```rust,ignore
use clear_signing::{
    format_calldata, resolve_descriptors_for_tx, EmptyDataProvider, TransactionContext,
};

let tx = TransactionContext {
    chain_id: 1,
    to: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
    calldata: &calldata_bytes,
    value: None,
    from: None,
    implementation_address: None,
};

let provider = EmptyDataProvider;
let descriptors = resolve_descriptors_for_tx(&tx, &registry_source, &provider).await?;
let outcome = format_calldata(&descriptors, &tx, &provider).await?;
```

Full API documentation: <https://docs.rs/clear-signing>.

## What's in the GitHub repo

This crate is the Rust core. The [main repository](https://github.com/llbartekll/clear-signing) also ships:

- A Swift Package + XCFramework for iOS
- An Android Kotlin AAR (UniFFI bindings)
- A SwiftUI sample wallet demonstrating WalletConnect + clear signing
- Integration guides

## Spec

Implements [ERC-7730 v2](https://eips.ethereum.org/EIPS/eip-7730).

## License

Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE-APACHE), at your option.
