//! Token metadata types and built-in providers.
//! Uses CAIP-19 keys (`eip155:{chain}/erc20:{addr}`) for cross-chain lookups.

use std::future::Future;
use std::pin::Pin;

use crate::provider::DataProvider;

/// Token metadata.
#[derive(Debug, Clone)]
pub struct TokenMeta {
    pub symbol: String,
    pub decimals: u8,
    pub name: String,
}

/// Normalized token lookup key (CAIP-19 style: `eip155:{chain_id}/erc20:{address}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenLookupKey(pub String);

impl TokenLookupKey {
    /// Create a lookup key from chain ID and address.
    pub fn new(chain_id: u64, address: &str) -> Self {
        let addr = address.to_lowercase();
        Self(format!("eip155:{chain_id}/erc20:{addr}"))
    }
}

/// Well-known token source with embedded metadata for common tokens.
pub struct WellKnownTokenSource {
    tokens: std::collections::HashMap<TokenLookupKey, TokenMeta>,
}

impl WellKnownTokenSource {
    pub fn new() -> Self {
        let json_str = include_str!("assets/tokens.json");
        let raw: std::collections::HashMap<String, WellKnownEntry> =
            serde_json::from_str(json_str).expect("embedded tokens.json is valid");
        let mut tokens = std::collections::HashMap::new();
        for (key, entry) in raw {
            tokens.insert(
                TokenLookupKey(key),
                TokenMeta {
                    symbol: entry.symbol,
                    decimals: entry.decimals,
                    name: entry.name,
                },
            );
        }
        Self { tokens }
    }
}

impl Default for WellKnownTokenSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataProvider for WellKnownTokenSource {
    fn resolve_token(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let key = TokenLookupKey::new(chain_id, address);
        let result = self.tokens.get(&key).cloned();
        Box::pin(async move { result })
    }
}

#[derive(serde::Deserialize)]
struct WellKnownEntry {
    symbol: String,
    decimals: u8,
    name: String,
}

/// Composite data provider that chains multiple providers, returning the first match.
pub struct CompositeDataProvider {
    providers: Vec<Box<dyn DataProvider>>,
}

impl CompositeDataProvider {
    pub fn new(providers: Vec<Box<dyn DataProvider>>) -> Self {
        Self { providers }
    }
}

impl DataProvider for CompositeDataProvider {
    fn resolve_token(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let address = address.to_string();
        Box::pin(async move {
            for provider in &self.providers {
                if let Some(meta) = provider.resolve_token(chain_id, &address).await {
                    return Some(meta);
                }
            }
            None
        })
    }

    fn resolve_ens_name(
        &self,
        address: &str,
        chain_id: u64,
        types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let address = address.to_string();
        let types_owned: Option<Vec<String>> = types.map(|t| t.to_vec());
        Box::pin(async move {
            for provider in &self.providers {
                if let Some(name) = provider
                    .resolve_ens_name(&address, chain_id, types_owned.as_deref())
                    .await
                {
                    return Some(name);
                }
            }
            None
        })
    }

    fn resolve_local_name(
        &self,
        address: &str,
        chain_id: u64,
        types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let address = address.to_string();
        let types_owned: Option<Vec<String>> = types.map(|t| t.to_vec());
        Box::pin(async move {
            for provider in &self.providers {
                if let Some(name) = provider
                    .resolve_local_name(&address, chain_id, types_owned.as_deref())
                    .await
                {
                    return Some(name);
                }
            }
            None
        })
    }

    fn resolve_nft_collection_name(
        &self,
        collection_address: &str,
        chain_id: u64,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let collection_address = collection_address.to_string();
        Box::pin(async move {
            for provider in &self.providers {
                if let Some(name) = provider
                    .resolve_nft_collection_name(&collection_address, chain_id)
                    .await
                {
                    return Some(name);
                }
            }
            None
        })
    }
}

/// In-memory token source for testing.
pub struct StaticTokenSource {
    tokens: std::collections::HashMap<TokenLookupKey, TokenMeta>,
}

impl StaticTokenSource {
    pub fn new() -> Self {
        Self {
            tokens: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, chain_id: u64, address: &str, meta: TokenMeta) {
        self.tokens
            .insert(TokenLookupKey::new(chain_id, address), meta);
    }
}

impl Default for StaticTokenSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataProvider for StaticTokenSource {
    fn resolve_token(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let key = TokenLookupKey::new(chain_id, address);
        let result = self.tokens.get(&key).cloned();
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_well_known_usdc_mainnet() {
        let source = WellKnownTokenSource::new();
        let meta = source
            .resolve_token(1, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
            .await
            .expect("USDC should be in well-known tokens");
        assert_eq!(meta.symbol, "USDC");
        assert_eq!(meta.decimals, 6);
    }

    #[tokio::test]
    async fn test_well_known_usdc_base() {
        let source = WellKnownTokenSource::new();
        let meta = source
            .resolve_token(8453, "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")
            .await
            .expect("USDC on Base should be found");
        assert_eq!(meta.symbol, "USDC");
        assert_eq!(meta.decimals, 6);
    }

    #[tokio::test]
    async fn test_well_known_not_found() {
        let source = WellKnownTokenSource::new();
        assert!(source
            .resolve_token(1, "0x0000000000000000000000000000000000000001")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_composite_source_fallthrough() {
        let mut custom = StaticTokenSource::new();
        custom.insert(
            1,
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            TokenMeta {
                symbol: "CUSTOM_USDC".to_string(),
                decimals: 6,
                name: "Custom USDC".to_string(),
            },
        );

        let composite = CompositeDataProvider::new(vec![
            Box::new(custom),
            Box::new(WellKnownTokenSource::new()),
        ]);

        // Custom takes precedence
        let meta = composite
            .resolve_token(1, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
            .await
            .unwrap();
        assert_eq!(meta.symbol, "CUSTOM_USDC");

        // Falls through to well-known for tokens not in custom
        let meta2 = composite
            .resolve_token(1, "0xdac17f958d2ee523a2206206994597c13d831ec7")
            .await
            .unwrap();
        assert_eq!(meta2.symbol, "USDT");
    }
}
