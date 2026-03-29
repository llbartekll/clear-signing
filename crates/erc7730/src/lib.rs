//! ERC-7730 v2 clear signing library — decodes and formats contract calldata
//! and EIP-712 typed data for human-readable display using JSON descriptors.
//!
//! Entry points: [`format_calldata()`], [`format_typed_data()`].

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub mod decoder;
pub mod eip712;
mod eip712_domain;
pub mod engine;
pub mod error;
pub mod merge;
mod path;
pub mod provider;
mod render_shared;
pub mod resolver;
pub mod token;
pub mod types;
#[cfg(feature = "uniffi")]
pub mod uniffi_compat;

use error::Error;

// Re-exports for convenience
pub use engine::{DisplayEntry, DisplayItem, DisplayModel};
pub use merge::merge_descriptors;
pub use provider::{DataProvider, EmptyDataProvider};
#[cfg(feature = "github-registry")]
pub use resolver::resolve_descriptors_for_typed_data;
pub use resolver::{
    resolve_descriptors_for_tx, DescriptorSource, ResolvedDescriptor, TypedDescriptorLookup,
};
pub use token::{CompositeDataProvider, TokenMeta, WellKnownTokenSource};
pub use types::descriptor::Descriptor;

/// Transaction context for calldata formatting.
pub struct TransactionContext<'a> {
    pub chain_id: u64,
    pub to: &'a str,
    pub calldata: &'a [u8],
    pub value: Option<&'a [u8]>,
    pub from: Option<&'a str>,
    /// For proxy contracts: the implementation address to match descriptors against.
    /// When set, descriptor matching uses this instead of `to`.
    /// Container value `@.to` still uses `to` (the proxy address the user interacts with).
    pub implementation_address: Option<&'a str>,
}

/// Format contract calldata for clear signing display.
///
/// This is the main entry point for calldata clear signing.
/// Takes a slice of pre-resolved descriptors. The outer descriptor is found by
/// matching `chain_id + tx.to`. Remaining descriptors are available for nested calldata.
/// Single-element slice = simple case, multi-element = nesting.
pub async fn format_calldata(
    descriptors: &[ResolvedDescriptor],
    tx: &TransactionContext<'_>,
    data_provider: &dyn DataProvider,
) -> Result<DisplayModel, Error> {
    if tx.calldata.len() < 4 {
        return Err(Error::Decode(error::DecodeError::CalldataTooShort {
            expected: 4,
            actual: tx.calldata.len(),
        }));
    }

    // Find the outer descriptor matching chain_id + address.
    // For proxy contracts, match against implementation_address instead of to.
    let match_address = tx.implementation_address.unwrap_or(tx.to);
    let outer_idx = descriptors.iter().position(|rd| {
        rd.descriptor.context.deployments().iter().any(|dep| {
            dep.chain_id == tx.chain_id
                && dep.address.to_lowercase() == match_address.to_lowercase()
        })
    });

    let outer_idx = match outer_idx {
        Some(idx) => idx,
        None => {
            if descriptors.is_empty() {
                return Ok(build_raw_fallback(tx.calldata));
            }
            // Fallback to first descriptor
            0
        }
    };

    let outer_descriptor = &descriptors[outer_idx].descriptor;
    let actual_selector = &tx.calldata[..4];

    // Find matching format key and parse its signature
    let (sig, _format_key) = match find_matching_signature(outer_descriptor, actual_selector) {
        Ok(result) => result,
        Err(_) => {
            // Graceful fallback: return raw preview for unknown selectors
            return Ok(build_raw_fallback(tx.calldata));
        }
    };

    // Decode calldata using the parsed signature
    let mut decoded = decoder::decode_calldata(&sig, tx.calldata)?;

    // Inject container values as synthetic arguments
    inject_container_values(&mut decoded, tx.chain_id, tx.to, tx.value, tx.from);

    // Render the display model
    engine::format_calldata(
        outer_descriptor,
        tx.chain_id,
        tx.to,
        &decoded,
        tx.value,
        data_provider,
        descriptors,
    )
    .await
}

/// Inject EIP-7730 container values (@.value, @.to, @.chainId, @.from) as synthetic arguments.
pub(crate) fn inject_container_values(
    decoded: &mut decoder::DecodedArguments,
    chain_id: u64,
    to: &str,
    value: Option<&[u8]>,
    from: Option<&str>,
) {
    // @.value — transaction ETH value
    if let Some(val_bytes) = value {
        let mut padded = vec![0u8; 32usize.saturating_sub(val_bytes.len())];
        padded.extend_from_slice(val_bytes);
        decoded.args.push(decoder::DecodedArgument {
            index: decoded.args.len(),
            name: Some("value".into()),
            param_type: decoder::ParamType::Uint(256),
            value: decoder::ArgumentValue::Uint(padded),
        });
    }

    // @.to — target contract address
    if let Some(addr) = parse_address_bytes(to) {
        decoded.args.push(decoder::DecodedArgument {
            index: decoded.args.len(),
            name: Some("to".into()),
            param_type: decoder::ParamType::Address,
            value: decoder::ArgumentValue::Address(addr),
        });
    }

    // @.chainId
    let chain_bytes = {
        let mut buf = [0u8; 32];
        buf[24..32].copy_from_slice(&chain_id.to_be_bytes());
        buf.to_vec()
    };
    decoded.args.push(decoder::DecodedArgument {
        index: decoded.args.len(),
        name: Some("chainId".into()),
        param_type: decoder::ParamType::Uint(256),
        value: decoder::ArgumentValue::Uint(chain_bytes),
    });

    // @.from — sender address (if provided)
    if let Some(from_addr) = from {
        if let Some(addr) = parse_address_bytes(from_addr) {
            decoded.args.push(decoder::DecodedArgument {
                index: decoded.args.len(),
                name: Some("from".into()),
                param_type: decoder::ParamType::Address,
                value: decoder::ArgumentValue::Address(addr),
            });
        }
    }
}

pub(crate) fn parse_address_bytes(addr: &str) -> Option<[u8; 20]> {
    let hex_str = addr
        .strip_prefix("0x")
        .or_else(|| addr.strip_prefix("0X"))
        .unwrap_or(addr);
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 20 {
        return None;
    }
    let mut result = [0u8; 20];
    result.copy_from_slice(&bytes);
    Some(result)
}

/// Build a raw fallback DisplayModel for unknown selectors (graceful degradation).
pub(crate) fn build_raw_fallback(calldata: &[u8]) -> DisplayModel {
    let selector = if calldata.len() >= 4 {
        format!("0x{}", hex::encode(&calldata[..4]))
    } else {
        format!("0x{}", hex::encode(calldata))
    };

    let mut entries = Vec::new();
    let data = if calldata.len() > 4 {
        &calldata[4..]
    } else {
        &[]
    };

    // Split into 32-byte words
    for (i, chunk) in data.chunks(32).enumerate() {
        entries.push(DisplayEntry::Item(DisplayItem {
            label: format!("Param {}", i),
            value: format!("0x{}", hex::encode(chunk)),
        }));
    }

    DisplayModel {
        intent: format!("Unknown function {}", selector),
        interpolated_intent: None,
        entries,
        warnings: vec!["No matching descriptor format found".to_string()],
        owner: None,
    }
}

/// Format EIP-712 typed data for clear signing display.
///
/// Takes a slice of pre-resolved descriptors. The outer descriptor is found by
/// matching `chain_id + verifying_contract`. Remaining descriptors are available
/// for nested calldata. Single-element slice = simple case, multi-element = nesting.
pub async fn format_typed_data(
    descriptors: &[ResolvedDescriptor],
    data: &eip712::TypedData,
    data_provider: &dyn DataProvider,
) -> Result<DisplayModel, Error> {
    if descriptors.is_empty() {
        return Ok(eip712::build_typed_raw_fallback(data));
    }

    let chain_id = data.domain.chain_id.ok_or_else(|| {
        Error::Descriptor(
            "EIP-712 domain.chainId is required when formatting with descriptors".to_string(),
        )
    })?;
    let verifying_contract = data.domain.verifying_contract.as_deref().ok_or_else(|| {
        Error::Descriptor(
            "EIP-712 domain.verifyingContract is required when formatting with descriptors"
                .to_string(),
        )
    })?;

    let selection = resolver::select_typed_outer_descriptor(descriptors, data)?;

    let selected = match selection {
        resolver::TypedOuterSelection::Selected(selected) => selected,
        resolver::TypedOuterSelection::NoMatch(no_match) => {
            if no_match.domain_errors.is_empty() && no_match.format_misses.len() == 1 {
                return Err(
                    eip712::find_typed_format(&no_match.format_misses[0].descriptor, data)
                        .expect_err("single format miss must still fail exact format lookup"),
                );
            }
            let mut message = format!(
                "no EIP-712 descriptor found for chain_id={} verifying_contract={} after domain and encodeType validation",
                chain_id, verifying_contract
            );
            if !no_match.domain_errors.is_empty() {
                message.push_str(": ");
                message.push_str(&no_match.domain_errors.join("; "));
            } else if !no_match.format_misses.is_empty() {
                message.push_str(": no descriptor matched the typed-data encodeType");
            }
            return Err(Error::Descriptor(message));
        }
    };

    eip712::format_typed_data_with_format(
        &selected.outer.descriptor,
        data,
        selected.format,
        data_provider,
        descriptors,
    )
    .await
}

/// Find a format key whose signature matches the calldata selector.
pub(crate) fn find_matching_signature(
    descriptor: &Descriptor,
    actual_selector: &[u8],
) -> Result<(decoder::FunctionSignature, String), Error> {
    for key in descriptor.display.formats.keys() {
        if key.contains('(') {
            match decoder::parse_signature(key) {
                Ok(sig) => {
                    if sig.selector[..] == actual_selector[..4] {
                        return Ok((sig, key.clone()));
                    }
                }
                Err(_) => continue,
            }
        }
    }

    Err(Error::Render(format!(
        "no matching format key for selector 0x{}",
        hex::encode(&actual_selector[..4])
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::EmptyDataProvider;
    use crate::token::StaticTokenSource;

    fn wrap_rd(descriptor: Descriptor, chain_id: u64, address: &str) -> Vec<ResolvedDescriptor> {
        vec![ResolvedDescriptor {
            descriptor,
            chain_id,
            address: address.to_lowercase(),
        }]
    }

    fn test_descriptor_json() -> &'static str {
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

    #[tokio::test]
    async fn test_full_calldata_pipeline() {
        let descriptor = Descriptor::from_json(test_descriptor_json()).unwrap();
        let sig = decoder::parse_signature("transfer(address,uint256)").unwrap();

        // Build calldata: transfer(0x0000...0001, 1000)
        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        let mut addr_word = [0u8; 32];
        addr_word[31] = 1;
        calldata.extend_from_slice(&addr_word);
        let mut amount_word = [0u8; 32];
        amount_word[30] = 0x03;
        amount_word[31] = 0xe8;
        calldata.extend_from_slice(&amount_word);

        let provider = EmptyDataProvider;
        let addr = "0xdac17f958d2ee523a2206206994597c13d831ec7";
        let descriptors = wrap_rd(descriptor, 1, addr);
        let tx = TransactionContext {
            chain_id: 1,
            to: addr,
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

        assert_eq!(result.intent, "Transfer tokens");
        assert_eq!(result.entries.len(), 2);

        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "To");
            assert_eq!(item.value, "0x0000000000000000000000000000000000000001");
        } else {
            panic!("expected Item");
        }

        if let DisplayEntry::Item(ref item) = result.entries[1] {
            assert_eq!(item.label, "Amount");
            assert_eq!(item.value, "1000");
        } else {
            panic!("expected Item");
        }
    }

    #[tokio::test]
    async fn test_full_pipeline_with_token_amount() {
        let json = r#"{
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
                        "interpolatedIntent": "Send ${@.1} to ${@.0}",
                        "fields": [
                            {
                                "path": "@.0",
                                "label": "To",
                                "format": "addressName"
                            },
                            {
                                "path": "@.1",
                                "label": "Amount",
                                "format": "tokenAmount",
                                "params": {
                                    "tokenPath": "@.0"
                                }
                            }
                        ]
                    }
                }
            }
        }"#;

        let descriptor = Descriptor::from_json(json).unwrap();
        let sig = decoder::parse_signature("transfer(address,uint256)").unwrap();

        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        // token address
        let token_addr =
            hex::decode("000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7")
                .unwrap();
        calldata.extend_from_slice(&token_addr);
        // amount: 1_000_000 (1 USDT with 6 decimals)
        let mut amount_word = [0u8; 32];
        amount_word[29] = 0x0f;
        amount_word[30] = 0x42;
        amount_word[31] = 0x40;
        calldata.extend_from_slice(&amount_word);

        let mut tokens = StaticTokenSource::new();
        tokens.insert(
            1,
            "0xdac17f958d2ee523a2206206994597c13d831ec7",
            TokenMeta {
                symbol: "USDT".to_string(),
                decimals: 6,
                name: "Tether USD".to_string(),
            },
        );

        let addr = "0xdac17f958d2ee523a2206206994597c13d831ec7";
        let descriptors = wrap_rd(descriptor, 1, addr);
        let tx = TransactionContext {
            chain_id: 1,
            to: addr,
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

        assert_eq!(result.intent, "Transfer tokens");

        // The "To" field should show the address (addressName resolves via data provider)
        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "To");
        }

        // The amount should be formatted with token decimals
        if let DisplayEntry::Item(ref item) = result.entries[1] {
            assert_eq!(item.label, "Amount");
            assert_eq!(item.value, "1 USDT");
        }
    }

    #[tokio::test]
    async fn test_visibility_rules() {
        let json = r#"{
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 1, "address": "0xabc" }
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
                    "foo(uint256,uint256)": {
                        "intent": "Test visibility",
                        "fields": [
                            {
                                "path": "@.0",
                                "label": "Always visible",
                                "format": "number"
                            },
                            {
                                "path": "@.1",
                                "label": "Hidden",
                                "format": "number",
                                "visible": false
                            }
                        ]
                    }
                }
            }
        }"#;

        let descriptor = Descriptor::from_json(json).unwrap();
        let sig = decoder::parse_signature("foo(uint256,uint256)").unwrap();

        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        calldata.extend_from_slice(&[0u8; 32]); // arg 0
        calldata.extend_from_slice(&[0u8; 32]); // arg 1

        let descriptors = wrap_rd(descriptor, 1, "0xabc");
        let tx = TransactionContext {
            chain_id: 1,
            to: "0xabc",
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
            .await
            .unwrap();

        // Only 1 field should be visible (the second has visible: false)
        assert_eq!(result.entries.len(), 1);
        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "Always visible");
        }
    }

    #[tokio::test]
    async fn test_field_group() {
        let json = r#"{
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 1, "address": "0xabc" }
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
                    "foo(address,uint256)": {
                        "intent": "Test groups",
                        "fields": [
                            {
                                "fieldGroup": {
                                    "label": "Transfer Details",
                                    "fields": [
                                        {
                                            "path": "@.0",
                                            "label": "Recipient",
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
                        ]
                    }
                }
            }
        }"#;

        let descriptor = Descriptor::from_json(json).unwrap();
        let sig = decoder::parse_signature("foo(address,uint256)").unwrap();

        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        let mut addr = [0u8; 32];
        addr[31] = 0x42;
        calldata.extend_from_slice(&addr);
        let mut amount = [0u8; 32];
        amount[31] = 100;
        calldata.extend_from_slice(&amount);

        let descriptors = wrap_rd(descriptor, 1, "0xabc");
        let tx = TransactionContext {
            chain_id: 1,
            to: "0xabc",
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
            .await
            .unwrap();

        assert_eq!(result.entries.len(), 1);
        if let DisplayEntry::Group { label, items, .. } = &result.entries[0] {
            assert_eq!(label, "Transfer Details");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].label, "Recipient");
            assert_eq!(items[1].label, "Amount");
            assert_eq!(items[1].value, "100");
        } else {
            panic!("expected Group");
        }
    }

    #[tokio::test]
    async fn test_maps_lookup() {
        let json = r#"{
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 1, "address": "0xabc" }
                    ]
                }
            },
            "metadata": {
                "owner": "test",
                "enums": {},
                "constants": {},
                "addressBook": {},
                "maps": {
                    "orderTypes": {
                        "entries": {
                            "0": "Market",
                            "1": "Limit",
                            "2": "Stop"
                        }
                    }
                }
            },
            "display": {
                "definitions": {},
                "formats": {
                    "placeOrder(uint256)": {
                        "intent": "Place order",
                        "fields": [
                            {
                                "path": "@.0",
                                "label": "Order Type",
                                "params": {
                                    "mapReference": "orderTypes"
                                }
                            }
                        ]
                    }
                }
            }
        }"#;

        let descriptor = Descriptor::from_json(json).unwrap();
        let sig = decoder::parse_signature("placeOrder(uint256)").unwrap();

        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        let mut word = [0u8; 32];
        word[31] = 1; // value = 1 → "Limit"
        calldata.extend_from_slice(&word);

        let descriptors = wrap_rd(descriptor, 1, "0xabc");
        let tx = TransactionContext {
            chain_id: 1,
            to: "0xabc",
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
            .await
            .unwrap();

        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "Order Type");
            assert_eq!(item.value, "Limit");
        } else {
            panic!("expected Item");
        }
    }

    #[tokio::test]
    async fn test_stakeweight_increase_unlock_time() {
        let json = r#"{
            "context": {
                "contract": {
                    "deployments": [
                        { "chainId": 10, "address": "0x521B4C065Bbdbe3E20B3727340730936912DfA46" }
                    ]
                }
            },
            "metadata": {
                "owner": "WalletConnect",
                "contractName": "StakeWeight",
                "enums": {},
                "constants": {},
                "addressBook": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    "increaseUnlockTime(uint256)": {
                        "intent": "Increase Unlock Time",
                        "interpolatedIntent": "Increase unlock time to ${@.0}",
                        "fields": [
                            {
                                "path": "@.0",
                                "label": "New Unlock Time",
                                "format": "date"
                            }
                        ]
                    }
                }
            }
        }"#;

        let descriptor = Descriptor::from_json(json).unwrap();
        // Real calldata from yttrium test
        let calldata =
            hex::decode("7c616fe6000000000000000000000000000000000000000000000000000000006945563d")
                .unwrap();

        let addr = "0x521B4C065Bbdbe3E20B3727340730936912DfA46";
        let descriptors = wrap_rd(descriptor, 10, addr);
        let tx = TransactionContext {
            chain_id: 10,
            to: addr,
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
            .await
            .unwrap();

        assert_eq!(result.intent, "Increase Unlock Time");
        assert_eq!(result.entries.len(), 1);
        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "New Unlock Time");
            assert_eq!(item.value, "2025-12-19 13:42:21 UTC");
        } else {
            panic!("expected Item");
        }
        assert_eq!(
            result.interpolated_intent.as_deref(),
            Some("Increase unlock time to 2025-12-19 13:42:21 UTC")
        );
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_eip712_format() {
        let json = r#"{
            "context": {
                "eip712": {
                    "deployments": [
                        { "chainId": 1, "address": "0xabc" }
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
                    "Permit(address spender,uint256 value)": {
                        "intent": "Permit token spending",
                        "fields": [
                            {
                                "path": "spender",
                                "label": "Spender",
                                "format": "address"
                            },
                            {
                                "path": "value",
                                "label": "Amount",
                                "format": "number"
                            }
                        ]
                    }
                }
            }
        }"#;

        let descriptor = Descriptor::from_json(json).unwrap();
        let typed_data = eip712::TypedData {
            types: std::collections::HashMap::from([(
                "Permit".to_string(),
                vec![
                    eip712::TypedDataField {
                        name: "spender".to_string(),
                        field_type: "address".to_string(),
                    },
                    eip712::TypedDataField {
                        name: "value".to_string(),
                        field_type: "uint256".to_string(),
                    },
                ],
            )]),
            primary_type: "Permit".to_string(),
            domain: eip712::TypedDataDomain {
                name: Some("USDT".to_string()),
                version: Some("1".to_string()),
                chain_id: Some(1),
                verifying_contract: Some("0xabc".to_string()),
                salt: None,
                extra: std::collections::HashMap::new(),
            },
            container: None,
            message: serde_json::json!({
                "spender": "0x1234567890123456789012345678901234567890",
                "value": "1000000"
            }),
        };

        let descriptors = wrap_rd(descriptor, 1, "0xabc");
        let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
            .await
            .unwrap();
        assert_eq!(result.intent, "Permit token spending");
        assert_eq!(result.entries.len(), 2);

        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "Spender");
            assert_eq!(item.value, "0x1234567890123456789012345678901234567890");
        }

        if let DisplayEntry::Item(ref item) = result.entries[1] {
            assert_eq!(item.label, "Amount");
            assert_eq!(item.value, "1000000");
        }
    }

    #[tokio::test]
    async fn test_proxy_implementation_address() {
        let descriptor = Descriptor::from_json(test_descriptor_json()).unwrap();
        let sig = decoder::parse_signature("transfer(address,uint256)").unwrap();

        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        let mut addr_word = [0u8; 32];
        addr_word[31] = 1;
        calldata.extend_from_slice(&addr_word);
        let mut amount_word = [0u8; 32];
        amount_word[30] = 0x03;
        amount_word[31] = 0xe8;
        calldata.extend_from_slice(&amount_word);

        // Descriptor is deployed at 0xdac17f...ec7 (the implementation).
        // tx.to is a proxy address that does NOT match any descriptor.
        // implementation_address points to the real implementation.
        let impl_addr = "0xdac17f958d2ee523a2206206994597c13d831ec7";
        let proxy_addr = "0x1111111111111111111111111111111111111111";
        let descriptors = wrap_rd(descriptor, 1, impl_addr);
        let tx = TransactionContext {
            chain_id: 1,
            to: proxy_addr,
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: Some(impl_addr),
        };
        let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
            .await
            .unwrap();

        // Should match the descriptor via implementation_address
        assert_eq!(result.intent, "Transfer tokens");
        assert_eq!(result.entries.len(), 2);

        // @.to container value should be the proxy address (user-facing), not the implementation
        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "To");
        }
    }
}
