use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{
    eip712::TypedData, error::Error, provider::DataProvider, resolver::ResolvedDescriptor,
    token::TokenMeta, types::descriptor::Descriptor, DisplayModel,
};

#[cfg(feature = "github-registry")]
use crate::resolver::{DescriptorSource, GitHubRegistrySource};

#[cfg(feature = "github-registry")]
const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/llbartekll/7730-v2-registry/main";

#[cfg(feature = "github-registry")]
static REGISTRY_SOURCE: tokio::sync::OnceCell<GitHubRegistrySource> =
    tokio::sync::OnceCell::const_new();

#[cfg(feature = "github-registry")]
async fn get_registry_source() -> Result<&'static GitHubRegistrySource, FfiError> {
    REGISTRY_SOURCE
        .get_or_try_init(|| async {
            GitHubRegistrySource::from_registry(DEFAULT_REGISTRY_URL)
                .await
                .map_err(|e| FfiError::Resolve(format!("failed to initialize registry: {e}")))
        })
        .await
}

// ---------------------------------------------------------------------------
// FFI-safe token metadata record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TokenMetaFfi {
    pub symbol: String,
    pub decimals: u8,
    pub name: String,
}

impl From<TokenMetaFfi> for TokenMeta {
    fn from(ffi: TokenMetaFfi) -> Self {
        TokenMeta {
            symbol: ffi.symbol,
            decimals: ffi.decimals,
            name: ffi.name,
        }
    }
}

impl From<TokenMeta> for TokenMetaFfi {
    fn from(meta: TokenMeta) -> Self {
        TokenMetaFfi {
            symbol: meta.symbol,
            decimals: meta.decimals,
            name: meta.name,
        }
    }
}

// ---------------------------------------------------------------------------
// Foreign data-provider trait (wallet implements this in Swift/Kotlin)
// ---------------------------------------------------------------------------

/// Sync callback trait for wallet-side data resolution.
///
/// Wallets implement this protocol (Swift/Kotlin) to provide token metadata,
/// ENS names, local contact names, and NFT collection names during clear-sign
/// formatting. Methods are synchronous across the FFI boundary — the proxy
/// bridges them to the async `DataProvider` trait used internally.
#[uniffi::export(with_foreign)]
pub trait DataProviderFfi: Send + Sync {
    fn resolve_token(&self, chain_id: u64, address: String) -> Option<TokenMetaFfi>;
    fn resolve_ens_name(
        &self,
        address: String,
        chain_id: u64,
        types: Option<Vec<String>>,
    ) -> Option<String>;
    fn resolve_local_name(
        &self,
        address: String,
        chain_id: u64,
        types: Option<Vec<String>>,
    ) -> Option<String>;
    fn resolve_nft_collection_name(
        &self,
        collection_address: String,
        chain_id: u64,
    ) -> Option<String>;
    /// Detect proxy contract implementation address.
    ///
    /// Called when descriptor resolution by `tx.to` fails. Wallets should read
    /// EIP-1967 implementation slot and/or Safe storage slot 0 via `eth_getStorageAt`.
    /// Return `None` if the address is not a known proxy.
    fn get_implementation_address(&self, chain_id: u64, address: String) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Proxy: wraps Arc<dyn DataProviderFfi> → implements internal DataProvider
// ---------------------------------------------------------------------------

pub struct DataProviderFfiProxy(pub Arc<dyn DataProviderFfi>);

impl DataProvider for DataProviderFfiProxy {
    fn resolve_token(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let address = address.to_string();
        let inner = Arc::clone(&self.0);
        Box::pin(async move {
            let result =
                tokio::task::spawn_blocking(move || inner.resolve_token(chain_id, address)).await;
            result.ok().flatten().map(Into::into)
        })
    }

    fn resolve_ens_name(
        &self,
        address: &str,
        chain_id: u64,
        types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let address = address.to_string();
        let types_owned = types.map(|t| t.to_vec());
        let inner = Arc::clone(&self.0);
        Box::pin(async move {
            let result = tokio::task::spawn_blocking(move || {
                inner.resolve_ens_name(address, chain_id, types_owned)
            })
            .await;
            result.ok().flatten()
        })
    }

    fn resolve_local_name(
        &self,
        address: &str,
        chain_id: u64,
        types: Option<&[String]>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let address = address.to_string();
        let types_owned = types.map(|t| t.to_vec());
        let inner = Arc::clone(&self.0);
        Box::pin(async move {
            let result = tokio::task::spawn_blocking(move || {
                inner.resolve_local_name(address, chain_id, types_owned)
            })
            .await;
            result.ok().flatten()
        })
    }

    fn resolve_nft_collection_name(
        &self,
        collection_address: &str,
        chain_id: u64,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        let collection_address = collection_address.to_string();
        let inner = Arc::clone(&self.0);
        Box::pin(async move {
            let result = tokio::task::spawn_blocking(move || {
                inner.resolve_nft_collection_name(collection_address, chain_id)
            })
            .await;
            result.ok().flatten()
        })
    }
}

// ---------------------------------------------------------------------------
// TransactionInput — FFI-safe transaction record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TransactionInput {
    pub chain_id: u64,
    pub to: String,
    pub calldata_hex: String,
    pub value_hex: Option<String>,
    pub from_address: Option<String>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Enum)]
pub enum FfiError {
    #[error("invalid descriptor JSON: {0}")]
    InvalidDescriptorJson(String),
    #[error("invalid typed data JSON: {0}")]
    InvalidTypedDataJson(String),
    #[error("invalid calldata hex: {0}")]
    InvalidCalldataHex(String),
    #[error("invalid value hex: {0}")]
    InvalidValueHex(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("descriptor error: {0}")]
    Descriptor(String),
    #[error("resolve error: {0}")]
    Resolve(String),
    #[error("token registry error: {0}")]
    TokenRegistry(String),
    #[error("render error: {0}")]
    Render(String),
}

impl From<Error> for FfiError {
    fn from(value: Error) -> Self {
        match value {
            Error::Decode(err) => Self::Decode(err.to_string()),
            Error::Descriptor(err) => Self::Descriptor(err),
            Error::Resolve(err) => Self::Resolve(err.to_string()),
            Error::TokenRegistry(err) => Self::TokenRegistry(err),
            Error::Render(err) => Self::Render(err),
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Format contract calldata for clear signing display.
///
/// Takes pre-resolved descriptor JSON strings and a `TransactionInput`.
/// The wallet is responsible for descriptor resolution (via `erc7730_resolve_descriptor`
/// or its own source). Proxy detection is automatic when `data_provider` is provided.
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_format_calldata(
    descriptors_json: Vec<String>,
    transaction: TransactionInput,
    data_provider: Option<Arc<dyn DataProviderFfi>>,
) -> Result<DisplayModel, FfiError> {
    let descriptors = parse_descriptors(&descriptors_json, transaction.chain_id, &transaction.to)?;
    let calldata = decode_hex(&transaction.calldata_hex, HexContext::Calldata)?;
    let value = match transaction.value_hex {
        Some(ref hex_value) => Some(decode_hex(hex_value, HexContext::Value)?),
        None => None,
    };
    // Auto-detect proxy implementation address for descriptor matching.
    let impl_addr = data_provider
        .as_ref()
        .and_then(|dp| dp.get_implementation_address(transaction.chain_id, transaction.to.clone()));
    let provider = build_data_provider(data_provider);
    let tx = crate::TransactionContext {
        chain_id: transaction.chain_id,
        to: &transaction.to,
        calldata: &calldata,
        value: value.as_deref(),
        from: transaction.from_address.as_deref(),
        implementation_address: impl_addr.as_deref(),
    };
    crate::format_calldata(&descriptors, &tx, provider.as_ref())
        .await
        .map_err(Into::into)
}

/// Format EIP-712 typed data for clear signing display.
///
/// Takes pre-resolved descriptor JSON strings and the EIP-712 typed data JSON.
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_format_typed_data(
    descriptors_json: Vec<String>,
    typed_data_json: String,
    data_provider: Option<Arc<dyn DataProviderFfi>>,
) -> Result<DisplayModel, FfiError> {
    println!("[erc7730] format_typed_data: descriptors_count={}, typed_data_len={}", descriptors_json.len(), typed_data_json.len());
    let typed_data: TypedData = serde_json::from_str::<TypedData>(&typed_data_json)
        .map_err(|e| FfiError::InvalidTypedDataJson(e.to_string()))?;

    let chain_id = typed_data.domain.chain_id.unwrap_or(1);
    let address = typed_data
        .domain
        .verifying_contract
        .as_deref()
        .unwrap_or("0x0000000000000000000000000000000000000000");
    println!("[erc7730] format_typed_data: chain_id={}, verifying_contract={}, primaryType={}", chain_id, address, typed_data.primary_type);
    let descriptors = parse_descriptors(&descriptors_json, chain_id, address)?;
    println!("[erc7730] format_typed_data: parsed {} descriptors", descriptors.len());
    for (i, rd) in descriptors.iter().enumerate() {
        let dep_addrs: Vec<String> = rd.descriptor.context.deployments().iter().map(|d| format!("{}:{}", d.chain_id, &d.address)).collect();
        let format_keys: Vec<&String> = rd.descriptor.display.formats.keys().collect();
        println!("[erc7730] format_typed_data: descriptor[{}] deployments={:?}, format_keys={:?}", i, dep_addrs, format_keys);
    }

    let provider = build_data_provider(data_provider);
    let result = crate::format_typed_data(&descriptors, &typed_data, provider.as_ref())
        .await
        .map_err(Into::into);
    match &result {
        Ok(model) => println!("[erc7730] format_typed_data: SUCCESS intent={:?}, warnings={:?}, entries={}", model.intent, model.warnings, model.entries.len()),
        Err(e) => println!("[erc7730] format_typed_data: ERROR {:?}", e),
    }
    result
}

/// Resolve a calldata descriptor from the GitHub registry for a given chain + address.
///
/// Returns the descriptor JSON string, or `None` if no descriptor is found.
/// Requires the `github-registry` feature.
#[cfg(feature = "github-registry")]
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_resolve_descriptor(
    chain_id: u64,
    address: String,
) -> Result<Option<String>, FfiError> {
    let source = get_registry_source().await?;
    match source.resolve_calldata(chain_id, &address).await {
        Ok(resolved) => {
            let json = serde_json::to_string(&resolved.descriptor)
                .map_err(|e| FfiError::Descriptor(e.to_string()))?;
            Ok(Some(json))
        }
        Err(crate::error::ResolveError::NotFound { .. }) => Ok(None),
        Err(e) => Err(FfiError::Resolve(e.to_string())),
    }
}

/// Resolve an EIP-712 descriptor from the GitHub registry for typed data.
///
/// Looks up by `chain_id` + `verifying_contract`. Automatically detects proxy
/// contracts via `data_provider.get_implementation_address()` as fallback.
/// Returns the descriptor JSON string, or `None` if no descriptor is found.
/// Requires the `github-registry` feature.
#[cfg(feature = "github-registry")]
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_resolve_descriptor_for_typed_data(
    chain_id: u64,
    verifying_contract: String,
    data_provider: Arc<dyn DataProviderFfi>,
) -> Result<Option<String>, FfiError> {
    println!("[erc7730] resolve_descriptor_for_typed_data: chain_id={}, verifying_contract={}", chain_id, verifying_contract);
    let source = get_registry_source().await?;

    // Try direct lookup by verifying contract
    match source.resolve_typed(chain_id, &verifying_contract).await {
        Ok(resolved) => {
            let json = serde_json::to_string(&resolved.descriptor)
                .map_err(|e| FfiError::Descriptor(e.to_string()))?;
            println!("[erc7730] resolve_typed_data: FOUND descriptor directly for {}", verifying_contract);
            return Ok(Some(json));
        }
        Err(crate::error::ResolveError::NotFound { .. }) => {
            println!("[erc7730] resolve_typed_data: NOT FOUND for {}, trying proxy detection", verifying_contract);
        }
        Err(e) => {
            println!("[erc7730] resolve_typed_data: ERROR for {}: {}", verifying_contract, e);
            return Err(FfiError::Resolve(e.to_string()));
        }
    }

    // Proxy detection fallback
    let impl_addr = data_provider.get_implementation_address(chain_id, verifying_contract.clone());
    println!("[erc7730] resolve_typed_data: get_implementation_address({}, {}) = {:?}", chain_id, verifying_contract, impl_addr);

    if let Some(impl_addr) = impl_addr {
        match source.resolve_typed(chain_id, &impl_addr).await {
            Ok(resolved) => {
                let json = serde_json::to_string(&resolved.descriptor)
                    .map_err(|e| FfiError::Descriptor(e.to_string()))?;
                println!("[erc7730] resolve_typed_data: FOUND descriptor via proxy impl_addr={}", impl_addr);
                return Ok(Some(json));
            }
            Err(crate::error::ResolveError::NotFound { .. }) => {
                println!("[erc7730] resolve_typed_data: NOT FOUND for impl_addr={} either", impl_addr);
            }
            Err(e) => {
                println!("[erc7730] resolve_typed_data: ERROR for impl_addr={}: {}", impl_addr, e);
                return Err(FfiError::Resolve(e.to_string()));
            }
        }
    }

    println!("[erc7730] resolve_typed_data: returning None (no descriptor found)");
    Ok(None)
}

/// Resolve all descriptors needed for a transaction, including nested calldata.
///
/// Uses the GitHub registry. Returns descriptor JSON strings in dependency order.
/// First element is the outer descriptor, subsequent are inner callees.
/// Returns empty vec if no descriptor is found for the outer address.
/// Automatically detects proxy contracts via `data_provider.get_implementation_address`.
#[cfg(feature = "github-registry")]
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_resolve_descriptors_for_tx(
    transaction: TransactionInput,
    data_provider: Arc<dyn DataProviderFfi>,
) -> Result<Vec<String>, FfiError> {
    println!("[erc7730] resolve_descriptors_for_tx: chain_id={}, to={}", transaction.chain_id, transaction.to);
    let source = get_registry_source().await?;
    let calldata = decode_hex(&transaction.calldata_hex, HexContext::Calldata)?;
    let value = match transaction.value_hex {
        Some(ref hex_value) => Some(decode_hex(hex_value, HexContext::Value)?),
        None => None,
    };
    let tx = crate::TransactionContext {
        chain_id: transaction.chain_id,
        to: &transaction.to,
        calldata: &calldata,
        value: value.as_deref(),
        from: transaction.from_address.as_deref(),
        implementation_address: None,
    };
    let mut descriptors = crate::resolve_descriptors_for_tx(&tx, source)
        .await
        .map_err(|e| FfiError::Resolve(e.to_string()))?;
    println!("[erc7730] resolve_descriptors_for_tx: direct lookup found {} descriptors", descriptors.len());

    // Proxy detection fallback: if no descriptors found, ask the wallet to detect
    // the proxy's implementation address and retry.
    if descriptors.is_empty() {
        let impl_addr = data_provider.get_implementation_address(transaction.chain_id, transaction.to.clone());
        println!("[erc7730] resolve_descriptors_for_tx: proxy detection impl_addr={:?}", impl_addr);
        if let Some(impl_addr) = impl_addr {
            let tx_with_impl = crate::TransactionContext {
                implementation_address: Some(impl_addr.as_str()),
                ..tx
            };
            descriptors = crate::resolve_descriptors_for_tx(&tx_with_impl, source)
                .await
                .map_err(|e| FfiError::Resolve(e.to_string()))?;
            println!("[erc7730] resolve_descriptors_for_tx: proxy retry found {} descriptors", descriptors.len());
        }
    }

    println!("[erc7730] resolve_descriptors_for_tx: returning {} descriptors total", descriptors.len());
    descriptors
        .iter()
        .map(|rd| {
            serde_json::to_string(&rd.descriptor).map_err(|e| FfiError::Descriptor(e.to_string()))
        })
        .collect()
}

/// Merge two descriptor JSON strings (including + included).
///
/// Returns merged JSON ready for use with `erc7730_format_calldata` / `erc7730_format_typed_data`.
#[uniffi::export]
pub fn erc7730_merge_descriptors(
    including_json: String,
    included_json: String,
) -> Result<String, FfiError> {
    crate::merge::merge_descriptors(&including_json, &included_json).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

enum HexContext {
    Calldata,
    Value,
}

fn decode_hex(input: &str, context: HexContext) -> Result<Vec<u8>, FfiError> {
    let trimmed = input.trim();
    let normalized = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    // Pad odd-length hex strings with a leading zero (e.g. "0x0" → "00")
    let padded;
    let hex_str = if normalized.len() % 2 != 0 {
        padded = format!("0{}", normalized);
        &padded
    } else {
        normalized
    };

    hex::decode(hex_str).map_err(|err| match context {
        HexContext::Calldata => FfiError::InvalidCalldataHex(err.to_string()),
        HexContext::Value => FfiError::InvalidValueHex(err.to_string()),
    })
}

fn parse_descriptors(
    descriptors_json: &[String],
    fallback_chain_id: u64,
    fallback_address: &str,
) -> Result<Vec<ResolvedDescriptor>, FfiError> {
    let mut descriptors = Vec::with_capacity(descriptors_json.len());
    for json_str in descriptors_json {
        let descriptor = Descriptor::from_json(json_str)
            .map_err(|e| FfiError::InvalidDescriptorJson(e.to_string()))?;
        let (cid, addr) = descriptor
            .context
            .deployments()
            .first()
            .map(|dep| (dep.chain_id, dep.address.clone()))
            .unwrap_or((fallback_chain_id, fallback_address.to_string()));
        descriptors.push(ResolvedDescriptor {
            descriptor,
            chain_id: cid,
            address: addr,
        });
    }
    Ok(descriptors)
}

fn build_data_provider(ffi_provider: Option<Arc<dyn DataProviderFfi>>) -> Box<dyn DataProvider> {
    match ffi_provider {
        Some(ffi) => Box::new(DataProviderFfiProxy(ffi)),
        None => Box::new(crate::provider::EmptyDataProvider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DisplayEntry;

    fn calldata_descriptor_json() -> &'static str {
        r#"{
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 1, "address": "0xdac17f958d2ee523a2206206994597c13d831ec7" }
                    ]
                }
            },
            "metadata": {
                "owner": "test",
                "contractName": "Tether USD",
                "enums": {},
                "constants": {},
                "addressBook": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    "transfer(address,uint256)": {
                        "intent": "Transfer tokens",
                        "fields": [
                            {
                                "path": "@.0",
                                "label": "To",
                                "format": "address"
                            },
                            {
                                "path": "@.1",
                                "label": "Amount",
                                "format": "number"
                            }
                        ]
                    }
                }
            }
        }"#
    }

    fn typed_descriptor_json() -> &'static str {
        r#"{
            "context": {
                "eip712": {
                    "deployments": [
                        { "chainId": 1, "address": "0x0000000000000000000000000000000000000001" }
                    ]
                }
            },
            "metadata": {
                "owner": "test",
                "enums": {},
                "constants": {},
                "addressBook": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    "Mail": {
                        "intent": "Sign mail",
                        "fields": [
                            {
                                "path": "@.from",
                                "label": "From",
                                "format": "address"
                            },
                            {
                                "path": "@.contents",
                                "label": "Contents",
                                "format": "raw"
                            }
                        ]
                    }
                }
            }
        }"#
    }

    fn typed_data_json() -> &'static str {
        r#"{
            "types": {
                "EIP712Domain": [
                    { "name": "chainId", "type": "uint256" },
                    { "name": "verifyingContract", "type": "address" }
                ],
                "Mail": [
                    { "name": "from", "type": "address" },
                    { "name": "contents", "type": "string" }
                ]
            },
            "primaryType": "Mail",
            "domain": {
                "chainId": 1,
                "verifyingContract": "0x0000000000000000000000000000000000000001"
            },
            "message": {
                "from": "0x0000000000000000000000000000000000000002",
                "contents": "hello"
            }
        }"#
    }

    fn transfer_calldata_hex() -> &'static str {
        "a9059cbb000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000003e8"
    }

    fn transfer_transaction() -> TransactionInput {
        TransactionInput {
            chain_id: 1,
            to: "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            calldata_hex: transfer_calldata_hex().to_string(),
            value_hex: None,
            from_address: None,
        }
    }

    #[tokio::test]
    async fn format_calldata_success() {
        let result = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            transfer_transaction(),
            None,
        )
        .await
        .expect("calldata formatting should succeed");

        assert_eq!(result.intent, "Transfer tokens");
        assert_eq!(result.entries.len(), 2);

        match &result.entries[0] {
            DisplayEntry::Item(item) => {
                assert_eq!(item.label, "To");
            }
            _ => {
                panic!("expected item entry");
            }
        }
    }

    #[tokio::test]
    async fn format_typed_success() {
        let result = erc7730_format_typed_data(
            vec![typed_descriptor_json().to_string()],
            typed_data_json().to_string(),
            None,
        )
        .await
        .expect("typed formatting should succeed");

        assert_eq!(result.intent, "Sign mail");
        assert_eq!(result.entries.len(), 2);
    }

    #[tokio::test]
    async fn format_calldata_invalid_descriptor_json() {
        let err = erc7730_format_calldata(vec!["{".to_string()], transfer_transaction(), None)
            .await
            .expect_err("invalid descriptor should fail");

        assert!(matches!(err, FfiError::InvalidDescriptorJson(_)));
    }

    #[tokio::test]
    async fn format_typed_invalid_typed_data_json() {
        let err = erc7730_format_typed_data(
            vec![typed_descriptor_json().to_string()],
            "{".to_string(),
            None,
        )
        .await
        .expect_err("invalid typed data should fail");

        assert!(matches!(err, FfiError::InvalidTypedDataJson(_)));
    }

    #[tokio::test]
    async fn format_calldata_invalid_calldata_hex() {
        let mut tx = transfer_transaction();
        tx.calldata_hex = "zz".to_string();

        let err = erc7730_format_calldata(vec![calldata_descriptor_json().to_string()], tx, None)
            .await
            .expect_err("invalid calldata hex should fail");

        assert!(matches!(err, FfiError::InvalidCalldataHex(_)));
    }

    #[tokio::test]
    async fn format_calldata_invalid_value_hex() {
        let mut tx = transfer_transaction();
        tx.value_hex = Some("zz".to_string());

        let err = erc7730_format_calldata(vec![calldata_descriptor_json().to_string()], tx, None)
            .await
            .expect_err("invalid value hex should fail");

        assert!(matches!(err, FfiError::InvalidValueHex(_)));
    }

    #[tokio::test]
    async fn format_calldata_accepts_0x_prefix() {
        let no_prefix = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            transfer_transaction(),
            None,
        )
        .await
        .expect("no-prefix calldata should succeed");

        let mut tx_with_prefix = transfer_transaction();
        tx_with_prefix.calldata_hex = format!("0x{}", transfer_calldata_hex());
        tx_with_prefix.value_hex = Some("0x00".to_string());

        let with_prefix = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            tx_with_prefix,
            None,
        )
        .await
        .expect("prefixed calldata should succeed");

        assert_eq!(no_prefix.intent, with_prefix.intent);
        assert_eq!(no_prefix.entries.len(), with_prefix.entries.len());
    }

    // -----------------------------------------------------------------------
    // Mock DataProviderFfi to validate end-to-end proxy wiring
    // -----------------------------------------------------------------------

    struct MockDataProviderFfi;

    impl DataProviderFfi for MockDataProviderFfi {
        fn resolve_token(&self, _chain_id: u64, _address: String) -> Option<TokenMetaFfi> {
            None
        }
        fn resolve_ens_name(
            &self,
            _address: String,
            _chain_id: u64,
            _types: Option<Vec<String>>,
        ) -> Option<String> {
            None
        }
        fn resolve_local_name(
            &self,
            address: String,
            _chain_id: u64,
            _types: Option<Vec<String>>,
        ) -> Option<String> {
            if address.to_lowercase() == "0x0000000000000000000000000000000000000001".to_lowercase()
            {
                Some("My Contact".to_string())
            } else {
                None
            }
        }
        fn resolve_nft_collection_name(
            &self,
            _collection_address: String,
            _chain_id: u64,
        ) -> Option<String> {
            None
        }
        fn get_implementation_address(&self, _chain_id: u64, _address: String) -> Option<String> {
            None
        }
    }

    #[tokio::test]
    async fn format_calldata_with_data_provider_ffi() {
        // Descriptor that uses addressName format (triggers local name resolution)
        let descriptor_json = r#"{
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 1, "address": "0xdac17f958d2ee523a2206206994597c13d831ec7" }
                    ]
                }
            },
            "metadata": {
                "owner": "test",
                "contractName": "Tether USD",
                "enums": {},
                "constants": {},
                "addressBook": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    "transfer(address,uint256)": {
                        "intent": "Transfer tokens",
                        "fields": [
                            {
                                "path": "@.0",
                                "label": "To",
                                "format": "addressName",
                                "params": {
                                    "sources": ["local"]
                                }
                            },
                            {
                                "path": "@.1",
                                "label": "Amount",
                                "format": "number"
                            }
                        ]
                    }
                }
            }
        }"#;

        let mock_provider: Arc<dyn DataProviderFfi> = Arc::new(MockDataProviderFfi);

        let result = erc7730_format_calldata(
            vec![descriptor_json.to_string()],
            transfer_transaction(),
            Some(mock_provider),
        )
        .await
        .expect("calldata formatting with data provider should succeed");

        assert_eq!(result.intent, "Transfer tokens");
        assert_eq!(result.entries.len(), 2);

        // The "To" address (0x...0001) should resolve to "My Contact" via mock provider
        match &result.entries[0] {
            DisplayEntry::Item(item) => {
                assert_eq!(item.label, "To");
                assert_eq!(item.value, "My Contact");
            }
            _ => panic!("expected item entry"),
        }
    }

    /// Simulates the exact wallet flow: descriptor JSON → serde round-trip → format_typed_data.
    /// Tests the encodeType format key matching through the FFI layer.
    #[tokio::test]
    async fn format_typed_data_velora_encode_type_key() {
        // Real descriptor from remote registry (with encodeType format key)
        let raw_descriptor_json = r#"{
            "context": {
                "eip712": {
                    "deployments": [
                        { "chainId": 1, "address": "0x0000000000bbf5c5fd284e657f01bd000933c96d" },
                        { "chainId": 10, "address": "0x0000000000bbf5c5fd284e657f01bd000933c96d" }
                    ],
                    "domain": { "name": "Portikus", "version": "2.0.0" }
                }
            },
            "metadata": { "owner": "Velora" },
            "display": {
                "formats": {
                    "Order(address owner,address beneficiary,address srcToken,address destToken,uint256 srcAmount,uint256 destAmount,uint256 expectedAmount,uint256 deadline,uint8 kind,uint256 nonce,uint256 partnerAndFee,bytes permit,bytes metadata,Bridge bridge)Bridge(bytes4 protocolSelector,uint256 destinationChainId,address outputToken,int8 scalingFactor,bytes protocolData)": {
                        "intent": "Swap order",
                        "fields": [
                            { "path": "srcAmount", "label": "Amount to send", "format": "tokenAmount", "params": { "tokenPath": "srcToken" } },
                            { "path": "destAmount", "label": "Minimum to receive", "format": "tokenAmount", "params": { "tokenPath": "destToken" } },
                            { "path": "beneficiary", "label": "Beneficiary", "format": "raw" },
                            { "path": "deadline", "label": "Expiration time", "format": "date", "params": { "encoding": "timestamp" } }
                        ]
                    }
                }
            }
        }"#;

        // Simulate the resolve round-trip: parse → serialize (like erc7730_resolve_descriptor does)
        let descriptor: Descriptor = serde_json::from_str(raw_descriptor_json).unwrap();
        let round_tripped_json = serde_json::to_string(&descriptor).unwrap();

        // Verify the format key survives round-trip
        assert!(
            round_tripped_json.contains("Order(address owner"),
            "encodeType key lost during serde round-trip: {}",
            round_tripped_json
        );

        let typed_data_json = r#"{
            "domain": {
                "chainId": 10,
                "name": "Portikus",
                "version": "2.0.0",
                "verifyingContract": "0x0000000000bbf5c5fd284e657f01bd000933c96d"
            },
            "message": {
                "owner": "0xbf01daf454dce008d3e2bfd47d5e186f71477253",
                "beneficiary": "0xbf01daf454dce008d3e2bfd47d5e186f71477253",
                "srcToken": "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58",
                "destToken": "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "srcAmount": "38627265",
                "destAmount": "18805928711910788",
                "expectedAmount": "18900430866241998",
                "deadline": 1774258780,
                "nonce": "1774258180237",
                "permit": "0x",
                "partnerAndFee": "90631063861114836560958097440945986548822432573276877133894239693005947666959",
                "bridge": {
                    "protocolSelector": "0x00000000",
                    "destinationChainId": 0,
                    "outputToken": "0x0000000000000000000000000000000000000000",
                    "scalingFactor": 0,
                    "protocolData": "0x"
                },
                "kind": 0,
                "metadata": "0x"
            },
            "primaryType": "Order",
            "types": {
                "EIP712Domain": [
                    { "name": "name", "type": "string" },
                    { "name": "version", "type": "string" },
                    { "name": "chainId", "type": "uint256" },
                    { "name": "verifyingContract", "type": "address" }
                ],
                "Order": [
                    { "name": "owner", "type": "address" },
                    { "name": "beneficiary", "type": "address" },
                    { "name": "srcToken", "type": "address" },
                    { "name": "destToken", "type": "address" },
                    { "name": "srcAmount", "type": "uint256" },
                    { "name": "destAmount", "type": "uint256" },
                    { "name": "expectedAmount", "type": "uint256" },
                    { "name": "deadline", "type": "uint256" },
                    { "name": "kind", "type": "uint8" },
                    { "name": "nonce", "type": "uint256" },
                    { "name": "partnerAndFee", "type": "uint256" },
                    { "name": "permit", "type": "bytes" },
                    { "name": "metadata", "type": "bytes" },
                    { "name": "bridge", "type": "Bridge" }
                ],
                "Bridge": [
                    { "name": "protocolSelector", "type": "bytes4" },
                    { "name": "destinationChainId", "type": "uint256" },
                    { "name": "outputToken", "type": "address" },
                    { "name": "scalingFactor", "type": "int8" },
                    { "name": "protocolData", "type": "bytes" }
                ]
            }
        }"#;

        // Call through the FFI function with the round-tripped descriptor
        let result =
            erc7730_format_typed_data(vec![round_tripped_json], typed_data_json.to_string(), None)
                .await
                .expect("typed data formatting should succeed");

        assert_eq!(result.intent, "Swap order");
        assert!(
            result.warnings.is_empty(),
            "unexpected warnings: {:?}",
            result.warnings
        );
        assert_eq!(result.entries.len(), 4);

        match &result.entries[0] {
            DisplayEntry::Item(item) => assert_eq!(item.label, "Amount to send"),
            _ => panic!("expected Item"),
        }
        match &result.entries[1] {
            DisplayEntry::Item(item) => assert_eq!(item.label, "Minimum to receive"),
            _ => panic!("expected Item"),
        }
        match &result.entries[2] {
            DisplayEntry::Item(item) => assert_eq!(item.label, "Beneficiary"),
            _ => panic!("expected Item"),
        }
        match &result.entries[3] {
            DisplayEntry::Item(item) => assert_eq!(item.label, "Expiration time"),
            _ => panic!("expected Item"),
        }
    }
}
