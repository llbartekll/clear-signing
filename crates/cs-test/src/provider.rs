use std::future::Future;
use std::pin::Pin;

use clear_signing::provider::DataProvider;
use clear_signing::token::TokenMeta;

use crate::schema::DataProviderStub;

pub struct StubDataProvider {
    stub: DataProviderStub,
}

impl StubDataProvider {
    pub fn new(stub: DataProviderStub) -> Self {
        Self { stub }
    }

    fn normalize(addr: &str) -> String {
        addr.trim_start_matches("0x").to_ascii_lowercase()
    }

    fn lookup_address_name(&self, address: &str) -> Option<String> {
        let key = Self::normalize(address);
        self.stub
            .address_names
            .iter()
            .find_map(|(k, v)| if Self::normalize(k) == key { Some(v.clone()) } else { None })
    }
}

impl DataProvider for StubDataProvider {
    fn resolve_token(
        &self,
        _chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let key = Self::normalize(address);
        let tok = self.stub.tokens.iter().find_map(|(k, v)| {
            if Self::normalize(k) == key {
                Some(TokenMeta {
                    symbol: v.symbol.clone(),
                    decimals: v.decimals,
                    name: v.name.clone().unwrap_or_else(|| v.symbol.clone()),
                })
            } else {
                None
            }
        });
        Box::pin(async move { tok })
    }

    fn resolve_ens_name(
        &self,
        address: &str,
        _chain_id: u64,
        _types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let name = self.lookup_address_name(address);
        Box::pin(async move { name })
    }

    fn resolve_local_name(
        &self,
        address: &str,
        _chain_id: u64,
        _types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let name = self.lookup_address_name(address);
        Box::pin(async move { name })
    }

    fn resolve_nft_collection_name(
        &self,
        collection_address: &str,
        _chain_id: u64,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let key = Self::normalize(collection_address);
        let name = self.stub.nft_collection_names.iter().find_map(|(k, v)| {
            if Self::normalize(k) == key {
                Some(v.clone())
            } else {
                None
            }
        });
        Box::pin(async move { name })
    }

    fn resolve_block_timestamp(
        &self,
        _chain_id: u64,
        block_number: u64,
    ) -> Pin<Box<dyn Future<Output = Option<u64>> + Send + '_>> {
        let key = block_number.to_string();
        let ts = self.stub.block_timestamps.get(&key).copied();
        Box::pin(async move { ts })
    }
}
