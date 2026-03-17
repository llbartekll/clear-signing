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
    fn resolve_ens_name(
        &self,
        address: &str,
        chain_id: u64,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let _ = (address, chain_id);
        Box::pin(async { None })
    }

    /// Resolve a local/contact name for an address.
    fn resolve_local_name(
        &self,
        address: &str,
        chain_id: u64,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let _ = (address, chain_id);
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
}

/// No-op data provider — all methods return `None`.
pub struct EmptyDataProvider;

impl DataProvider for EmptyDataProvider {}
