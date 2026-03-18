use crate::{
    eip712::TypedData,
    error::Error,
    resolver::ResolvedDescriptor,
    token::{StaticTokenSource, TokenMeta},
    types::descriptor::Descriptor,
    DisplayModel,
};

#[cfg(feature = "github-registry")]
use crate::resolver::{DescriptorSource, GitHubRegistrySource};

#[cfg(feature = "github-registry")]
use crate::token::{CompositeDataProvider, WellKnownTokenSource};

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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TokenMetaInput {
    pub chain_id: u64,
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
    pub name: String,
}

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

#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_format_calldata(
    descriptors_json: Vec<String>,
    chain_id: u64,
    to: String,
    calldata_hex: String,
    value_hex: Option<String>,
    from_address: Option<String>,
    tokens: Vec<TokenMetaInput>,
) -> Result<DisplayModel, FfiError> {
    let descriptors = parse_descriptors(&descriptors_json, chain_id, &to)?;

    let calldata = decode_hex(&calldata_hex, HexContext::Calldata)?;
    let value = match value_hex {
        Some(hex_value) => Some(decode_hex(&hex_value, HexContext::Value)?),
        None => None,
    };

    let token_source = build_token_source(&tokens);
    let tx = crate::TransactionContext {
        chain_id,
        to: &to,
        calldata: &calldata,
        value: value.as_deref(),
        from: from_address.as_deref(),
    };
    crate::format_calldata(&descriptors, &tx, &token_source)
        .await
        .map_err(Into::into)
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_format_typed_data(
    descriptors_json: Vec<String>,
    typed_data_json: String,
    tokens: Vec<TokenMetaInput>,
) -> Result<DisplayModel, FfiError> {
    let typed_data: TypedData = serde_json::from_str::<TypedData>(&typed_data_json)
        .map_err(|e| FfiError::InvalidTypedDataJson(e.to_string()))?;

    let chain_id = typed_data.domain.chain_id.unwrap_or(1);
    let address = typed_data
        .domain
        .verifying_contract
        .as_deref()
        .unwrap_or("0x0000000000000000000000000000000000000000");
    let descriptors = parse_descriptors(&descriptors_json, chain_id, address)?;

    let token_source = build_token_source(&tokens);
    crate::format_typed_data(&descriptors, &typed_data, &token_source)
        .await
        .map_err(Into::into)
}

/// High-level: resolve descriptor from GitHub registry, then format calldata.
///
/// Requires the `github-registry` feature.
#[cfg(feature = "github-registry")]
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_format(
    chain_id: u64,
    to: String,
    calldata_hex: String,
    value_hex: Option<String>,
    from_address: Option<String>,
    implementation_address: Option<String>,
    tokens: Vec<TokenMetaInput>,
) -> Result<DisplayModel, FfiError> {
    let source = get_registry_source().await?;
    let resolve_addr = implementation_address.as_deref().unwrap_or(&to);
    let resolved = match source.resolve_calldata(chain_id, resolve_addr).await {
        Ok(rd) => rd,
        Err(crate::error::ResolveError::NotFound { .. }) => {
            let calldata = decode_hex(&calldata_hex, HexContext::Calldata)?;
            return Ok(crate::build_raw_fallback(&calldata));
        }
        Err(e) => return Err(FfiError::Resolve(e.to_string())),
    };

    let calldata = decode_hex(&calldata_hex, HexContext::Calldata)?;
    let value = match value_hex {
        Some(hex_value) => Some(decode_hex(&hex_value, HexContext::Value)?),
        None => None,
    };

    let caller_tokens = build_token_source(&tokens);
    let well_known = WellKnownTokenSource::new();
    let composite = CompositeDataProvider::new(vec![Box::new(caller_tokens), Box::new(well_known)]);

    let tx = crate::TransactionContext {
        chain_id,
        to: &to,
        calldata: &calldata,
        value: value.as_deref(),
        from: from_address.as_deref(),
    };
    crate::format_calldata(&[resolved], &tx, &composite)
        .await
        .map_err(Into::into)
}

/// High-level: resolve descriptor from GitHub registry, then format EIP-712 typed data.
///
/// Requires the `github-registry` feature.
#[cfg(feature = "github-registry")]
#[uniffi::export(async_runtime = "tokio")]
pub async fn erc7730_format_typed(
    typed_data_json: String,
    tokens: Vec<TokenMetaInput>,
) -> Result<DisplayModel, FfiError> {
    let typed_data: TypedData = serde_json::from_str::<TypedData>(&typed_data_json)
        .map_err(|e| FfiError::InvalidTypedDataJson(e.to_string()))?;

    let chain_id = typed_data.domain.chain_id.unwrap_or(1);
    let address = typed_data
        .domain
        .verifying_contract
        .as_deref()
        .unwrap_or("0x0000000000000000000000000000000000000000");

    let source = get_registry_source().await?;
    let resolved = match source.resolve_typed(chain_id, address).await {
        Ok(rd) => rd,
        Err(crate::error::ResolveError::NotFound { .. }) => {
            return Ok(crate::eip712::build_typed_raw_fallback(&typed_data));
        }
        Err(e) => return Err(FfiError::Resolve(e.to_string())),
    };

    let caller_tokens = build_token_source(&tokens);
    let well_known = WellKnownTokenSource::new();
    let composite = CompositeDataProvider::new(vec![Box::new(caller_tokens), Box::new(well_known)]);

    crate::format_typed_data(&[resolved], &typed_data, &composite)
        .await
        .map_err(Into::into)
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

fn build_token_source(tokens: &[TokenMetaInput]) -> StaticTokenSource {
    let mut source = StaticTokenSource::new();
    for token in tokens {
        source.insert(
            token.chain_id,
            &token.address,
            TokenMeta {
                symbol: token.symbol.clone(),
                decimals: token.decimals,
                name: token.name.clone(),
            },
        );
    }
    source
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

    #[tokio::test]
    async fn format_calldata_success() {
        let result = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            transfer_calldata_hex().to_string(),
            None,
            None,
            vec![],
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
            vec![],
        )
        .await
        .expect("typed formatting should succeed");

        assert_eq!(result.intent, "Sign mail");
        assert_eq!(result.entries.len(), 2);
    }

    #[tokio::test]
    async fn format_calldata_invalid_descriptor_json() {
        let err = erc7730_format_calldata(
            vec!["{".to_string()],
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            transfer_calldata_hex().to_string(),
            None,
            None,
            vec![],
        )
        .await
        .expect_err("invalid descriptor should fail");

        assert!(matches!(err, FfiError::InvalidDescriptorJson(_)));
    }

    #[tokio::test]
    async fn format_typed_invalid_typed_data_json() {
        let err = erc7730_format_typed_data(
            vec![typed_descriptor_json().to_string()],
            "{".to_string(),
            vec![],
        )
        .await
        .expect_err("invalid typed data should fail");

        assert!(matches!(err, FfiError::InvalidTypedDataJson(_)));
    }

    #[tokio::test]
    async fn format_calldata_invalid_calldata_hex() {
        let err = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            "zz".to_string(),
            None,
            None,
            vec![],
        )
        .await
        .expect_err("invalid calldata hex should fail");

        assert!(matches!(err, FfiError::InvalidCalldataHex(_)));
    }

    #[tokio::test]
    async fn format_calldata_invalid_value_hex() {
        let err = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            transfer_calldata_hex().to_string(),
            Some("zz".to_string()),
            None,
            vec![],
        )
        .await
        .expect_err("invalid value hex should fail");

        assert!(matches!(err, FfiError::InvalidValueHex(_)));
    }

    #[tokio::test]
    async fn format_calldata_accepts_0x_prefix() {
        let no_prefix = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            transfer_calldata_hex().to_string(),
            None,
            None,
            vec![],
        )
        .await
        .expect("no-prefix calldata should succeed");

        let with_prefix = erc7730_format_calldata(
            vec![calldata_descriptor_json().to_string()],
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(),
            format!("0x{}", transfer_calldata_hex()),
            Some("0x00".to_string()),
            None,
            vec![],
        )
        .await
        .expect("prefixed calldata should succeed");

        assert_eq!(no_prefix.intent, with_prefix.intent);
        assert_eq!(no_prefix.entries.len(), with_prefix.entries.len());
    }
}
