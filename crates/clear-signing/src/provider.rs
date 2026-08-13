//! Unified async data provider trait for external data resolution.
//!
//! Wallets implement [`DataProvider`] to supply token metadata, address names,
//! and NFT collection names during formatting.

use std::future::Future;
use std::pin::Pin;

use crate::token::TokenMeta;

/// Async data provider for external data resolution during formatting.
///
/// Wallets implement this trait to supply token metadata, ENS/local address names,
/// and NFT collection names. All methods have default implementations returning `None`,
/// so implementors only need to override the methods they support.
pub trait DataProvider: Send + Sync {
    /// Resolve token metadata (symbol, decimals, name) for a given chain and address.
    fn resolve_token(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let _ = (chain_id, address);
        Box::pin(async { None })
    }

    /// Resolve an ENS name for an address.
    ///
    /// `types` hints the expected address role (e.g. `["eoa"]`, `["contract"]`).
    fn resolve_ens_name(
        &self,
        address: &str,
        chain_id: u64,
        types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let _ = (address, chain_id, types);
        Box::pin(async { None })
    }

    /// Resolve a local/contact name for an address.
    ///
    /// `types` hints the expected address role (e.g. `["eoa"]`, `["contract"]`).
    fn resolve_local_name(
        &self,
        address: &str,
        chain_id: u64,
        types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let _ = (address, chain_id, types);
        Box::pin(async { None })
    }

    /// Resolve an NFT collection name for a collection contract address.
    fn resolve_nft_collection_name(
        &self,
        collection_address: &str,
        chain_id: u64,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let _ = (collection_address, chain_id);
        Box::pin(async { None })
    }

    /// Resolve an approximate unix timestamp for a given block number.
    fn resolve_block_timestamp(
        &self,
        chain_id: u64,
        block_number: u64,
    ) -> Pin<Box<dyn Future<Output = Option<u64>> + Send + '_>> {
        let _ = (chain_id, block_number);
        Box::pin(async { None })
    }

    /// Decrypt a field value carrying an ERC-7730 `encryption` annotation.
    ///
    /// Optional and scheme-specific — decryption generally needs a live
    /// connection, a user signature, and an access-control check, so it lives in
    /// the wallet, not the library. `encrypted_value` is 0x-hex of the raw field
    /// bytes; `contract_address` is the container's `@.to` (absent for EIP-712
    /// domains without a `verifyingContract`).
    ///
    /// Return the plaintext as 0x-hex of its big-endian bytes; the library
    /// re-interprets it via the descriptor's declared `plaintextType`. Return
    /// `None` when the value cannot be decrypted (unsupported scheme, declined
    /// signature, no access) — the field then renders its `fallbackLabel`.
    fn resolve_decrypted_value(
        &self,
        chain_id: u64,
        encrypted_value: &str,
        scheme: &str,
        contract_address: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let _ = (chain_id, encrypted_value, scheme, contract_address);
        Box::pin(async { None })
    }
}

/// No-op data provider — all methods return `None`.
pub struct EmptyDataProvider;

impl DataProvider for EmptyDataProvider {}
