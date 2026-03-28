//! Tests for ERC-7730 spec compliance gaps identified in the audit.

use erc7730::decoder;
use erc7730::eip712::TypedData;
use erc7730::engine::{DisplayEntry, DisplayModel, GroupIteration};
use erc7730::merge::merge_descriptor_values;
use erc7730::provider::{DataProvider, EmptyDataProvider};
use erc7730::token::{StaticTokenSource, TokenMeta};
use erc7730::types::descriptor::Descriptor;
use erc7730::{
    format_calldata, format_typed_data, merge_descriptors, ResolvedDescriptor, TransactionContext,
};
use std::future::Future;
use std::pin::Pin;

fn wrap_rd(descriptor: Descriptor, chain_id: u64, address: &str) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor,
        chain_id,
        address: address.to_lowercase(),
    }]
}

fn build_calldata(sig_str: &str, words: &[[u8; 32]]) -> Vec<u8> {
    let sig = decoder::parse_signature(sig_str).unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    for word in words {
        calldata.extend_from_slice(word);
    }
    calldata
}

fn build_single_bytes_calldata(sig_str: &str, bytes: &[u8]) -> Vec<u8> {
    let sig = decoder::parse_signature(sig_str).unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);

    let mut offset = [0u8; 32];
    offset[31] = 0x20;
    calldata.extend_from_slice(&offset);

    let mut len = [0u8; 32];
    len[24..32].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
    calldata.extend_from_slice(&len);
    calldata.extend_from_slice(bytes);

    let padding = (32 - (bytes.len() % 32)) % 32;
    calldata.extend(std::iter::repeat_n(0u8, padding));
    calldata
}

fn uint_word(val: u64) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..32].copy_from_slice(&val.to_be_bytes());
    word
}

fn uint_hex_literal(val: u64) -> String {
    format!("0x{}", hex::encode(uint_word(val)))
}

fn addr_word(addr_hex: &str) -> [u8; 32] {
    let bytes = hex::decode(addr_hex.strip_prefix("0x").unwrap_or(addr_hex)).unwrap();
    let mut word = [0u8; 32];
    word[12..32].copy_from_slice(&bytes);
    word
}

fn dynamic_offset_word(offset: usize) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..32].copy_from_slice(&(offset as u64).to_be_bytes());
    word
}

fn encode_address_array(addrs: &[&str]) -> Vec<u8> {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&uint_word(addrs.len() as u64));
    for addr in addrs {
        encoded.extend_from_slice(&addr_word(addr));
    }
    encoded
}

fn encode_uint_array(values: &[u64]) -> Vec<u8> {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&uint_word(values.len() as u64));
    for value in values {
        encoded.extend_from_slice(&uint_word(*value));
    }
    encoded
}

fn build_two_array_calldata(sig_str: &str, addrs: &[&str], values: &[u64]) -> Vec<u8> {
    let sig = decoder::parse_signature(sig_str).unwrap();
    let addresses_encoded = encode_address_array(addrs);
    let values_encoded = encode_uint_array(values);
    let addresses_offset = 64usize;
    let values_offset = addresses_offset + addresses_encoded.len();

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&dynamic_offset_word(addresses_offset));
    calldata.extend_from_slice(&dynamic_offset_word(values_offset));
    calldata.extend_from_slice(&addresses_encoded);
    calldata.extend_from_slice(&values_encoded);
    calldata
}

struct BlockTimestampProvider(Option<u64>);

impl DataProvider for BlockTimestampProvider {
    fn resolve_block_timestamp(
        &self,
        _chain_id: u64,
        _block_number: u64,
    ) -> Pin<Box<dyn Future<Output = Option<u64>> + Send + '_>> {
        Box::pin(async move { self.0 })
    }
}

fn semantic_item_snapshot(entries: &[DisplayEntry]) -> Vec<(String, String)> {
    let mut snapshot = Vec::new();
    for entry in entries {
        match entry {
            DisplayEntry::Item(item) => {
                snapshot.push((item.label.clone(), item.value.clone()));
            }
            DisplayEntry::Group { items, .. } => {
                snapshot.extend(
                    items
                        .iter()
                        .map(|item| (item.label.clone(), item.value.clone())),
                );
            }
            DisplayEntry::Nested { label, intent, .. } => {
                snapshot.push((label.clone(), intent.clone()));
            }
        }
    }
    snapshot
}

fn assert_semantic_parity(calldata_model: &DisplayModel, typed_model: &DisplayModel) {
    assert_eq!(calldata_model.intent, typed_model.intent);
    assert_eq!(
        semantic_item_snapshot(&calldata_model.entries),
        semantic_item_snapshot(&typed_model.entries)
    );
}

async fn assert_invalid_typed_numeric_format_error(
    format_name: &str,
    params: Option<serde_json::Value>,
    bad_value: serde_json::Value,
    error_substr: &str,
) {
    let field = match params {
        Some(params) => serde_json::json!({
            "path": "value",
            "label": "Value",
            "format": format_name,
            "params": params
        }),
        None => serde_json::json!({
            "path": "value",
            "label": "Value",
            "format": format_name
        }),
    };

    let descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "Example(string value)": {
                        "intent": "Example",
                        "fields": [field]
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Example": [{ "name": "value", "type": "string" }]
        },
        "primaryType": "Example",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "value": bad_value }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains(error_substr),
        "expected '{error_substr}' in error, got: {err}"
    );
}

// ─── #3: Duplicate selector rejection ───

#[tokio::test]
async fn test_duplicate_selector_rejected() {
    // Two format keys that resolve to the same selector
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "transfer(address,uint256)": {
                    "intent": "Transfer 1",
                    "fields": [{"path": "@.0", "label": "To", "format": "address"}]
                },
                "transfer(address to,uint256 amount)": {
                    "intent": "Transfer 2",
                    "fields": [{"path": "to", "label": "To", "format": "address"}]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata(
        "transfer(address,uint256)",
        &[
            addr_word("0x0000000000000000000000000000000000000001"),
            uint_word(100),
        ],
    );

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider).await;
    // Should error due to duplicate selectors
    assert!(result.is_err(), "duplicate selectors should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("duplicate"),
        "error should mention duplicate: {err}"
    );
}

// ─── #1: EIP-712 Duration format ───

#[tokio::test]
async fn test_eip712_duration_format() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "Lock(uint256 duration)": {
                    "intent": "Lock tokens",
                    "fields": [
                        {"path": "duration", "label": "Duration", "format": "duration"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {"EIP712Domain": [], "Lock": [{"name": "duration", "type": "uint256"}]},
        "primaryType": "Lock",
        "domain": {"chainId": 1, "verifyingContract": "0xabc"},
        "message": {"duration": 90061}
    }))
    .unwrap();

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.value, "25:01:01");
    } else {
        panic!("expected Item");
    }
}

// ─── #1: EIP-712 Unit format ───

#[tokio::test]
async fn test_eip712_unit_format() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "SetRate(uint256 rate)": {
                    "intent": "Set rate",
                    "fields": [
                        {"path": "rate", "label": "Rate", "format": "unit", "params": {"base": "%", "decimals": 2}}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {"EIP712Domain": [], "SetRate": [{"name": "rate", "type": "uint256"}]},
        "primaryType": "SetRate",
        "domain": {"chainId": 1, "verifyingContract": "0xabc"},
        "message": {"rate": 1250}
    }))
    .unwrap();

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.value, "12.5%");
    } else {
        panic!("expected Item");
    }
}

// ─── #1: EIP-712 NftName format ───

#[tokio::test]
async fn test_eip712_nft_name_format() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "Transfer(uint256 tokenId)": {
                    "intent": "Transfer NFT",
                    "fields": [
                        {"path": "tokenId", "label": "Token", "format": "nftName", "params": {"collection": "0xdef"}}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {"EIP712Domain": [], "Transfer": [{"name": "tokenId", "type": "uint256"}]},
        "primaryType": "Transfer",
        "domain": {"chainId": 1, "verifyingContract": "0xabc"},
        "message": {"tokenId": 42}
    }))
    .unwrap();

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        // No collection resolver → fallback to "#42"
        assert_eq!(item.value, "#42");
    } else {
        panic!("expected Item");
    }
}

#[tokio::test]
async fn test_eip712_token_amount_rejects_invalid_numeric_string() {
    assert_invalid_typed_numeric_format_error(
        "tokenAmount",
        Some(serde_json::json!({
            "token": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        })),
        serde_json::json!("not-a-number"),
        "tokenAmount field must be an unsigned integer",
    )
    .await;
}

#[tokio::test]
async fn test_eip712_date_rejects_invalid_numeric_string() {
    assert_invalid_typed_numeric_format_error(
        "date",
        None,
        serde_json::json!("not-a-number"),
        "date field must be an integer",
    )
    .await;
}

#[tokio::test]
async fn test_eip712_chain_id_rejects_invalid_numeric_string() {
    assert_invalid_typed_numeric_format_error(
        "chainId",
        None,
        serde_json::json!("not-a-number"),
        "chainId field must be an unsigned integer",
    )
    .await;
}

#[tokio::test]
async fn test_eip712_duration_rejects_invalid_numeric_string() {
    assert_invalid_typed_numeric_format_error(
        "duration",
        None,
        serde_json::json!("not-a-number"),
        "duration field must be an unsigned integer",
    )
    .await;
}

#[tokio::test]
async fn test_eip712_unit_rejects_invalid_numeric_string() {
    assert_invalid_typed_numeric_format_error(
        "unit",
        Some(serde_json::json!({"base": "%", "decimals": 2})),
        serde_json::json!("not-a-number"),
        "unit field must be an unsigned integer",
    )
    .await;
}

// ─── #2: DisplayField with value (literal constant) ───

#[tokio::test]
async fn test_display_field_literal_value() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "transfer(address,uint256)": {
                    "intent": "Transfer",
                    "fields": [
                        {"value": "ERC-20 Transfer", "label": "Type"},
                        {"path": "@.0", "label": "To", "format": "address"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata(
        "transfer(address,uint256)",
        &[
            addr_word("0x0000000000000000000000000000000000000001"),
            uint_word(100),
        ],
    );

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
    assert_eq!(result.entries.len(), 2);
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Type");
        assert_eq!(item.value, "ERC-20 Transfer");
    } else {
        panic!("expected Item");
    }
}

// ─── #4: Separator for array elements ───

#[tokio::test]
async fn test_separator_for_array_field() {
    // Test that the separator field is parsed (deserialization test)
    let field_json = r#"{
        "path": "items",
        "label": "Items",
        "format": "raw",
        "separator": " | "
    }"#;
    let field: erc7730::types::display::DisplayField = serde_json::from_str(field_json).unwrap();
    if let erc7730::types::display::DisplayField::Simple { separator, .. } = &field {
        assert_eq!(separator.as_deref(), Some(" | "));
    } else {
        panic!("expected Simple");
    }
}

// ─── #6: Signed integer handling ───

#[tokio::test]
async fn test_signed_integer_negative() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "setDelta(int256)": {
                    "intent": "Set delta",
                    "fields": [
                        {"path": "@.0", "label": "Delta", "format": "number"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let sig = decoder::parse_signature("setDelta(int256)").unwrap();

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    // -1 in two's complement (32 bytes of 0xFF)
    calldata.extend_from_slice(&[0xFF; 32]);

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
        assert_eq!(item.value, "-1");
    } else {
        panic!("expected Item");
    }
}

#[tokio::test]
async fn test_signed_integer_negative_100() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "setDelta(int256)": {
                    "intent": "Set delta",
                    "fields": [
                        {"path": "@.0", "label": "Delta", "format": "number"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let sig = decoder::parse_signature("setDelta(int256)").unwrap();

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    // -100 in two's complement: 0xFFFFFFFF...FF9C
    let mut word = [0xFF; 32];
    word[31] = 0x9C; // 256 - 100 = 156 = 0x9C
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
        assert_eq!(item.value, "-100");
    } else {
        panic!("expected Item");
    }
}

// ─── #5: InteroperableAddressName stub ───

#[tokio::test]
async fn test_interoperable_address_name_deserialization() {
    let field_json = r#"{
        "path": "recipient",
        "label": "To",
        "format": "interoperableAddressName"
    }"#;
    let field: erc7730::types::display::DisplayField = serde_json::from_str(field_json).unwrap();
    if let erc7730::types::display::DisplayField::Simple { format, .. } = &field {
        assert!(matches!(
            format.as_ref(),
            Some(erc7730::types::display::FieldFormat::InteroperableAddressName)
        ));
    } else {
        panic!("expected Simple");
    }
}

// ─── #7: Date with blockheight encoding ───

#[tokio::test]
async fn test_date_blockheight_encoding() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "expireAt(uint256)": {
                    "intent": "Expire",
                    "fields": [
                        {"path": "@.0", "label": "Block", "format": "date", "params": {"encoding": "blockheight"}}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata("expireAt(uint256)", &[uint_word(19500000)]);

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(
        &descriptors,
        &tx,
        &BlockTimestampProvider(Some(1_710_000_000)),
    )
    .await
    .unwrap();
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.value, "2024-03-09 16:00:00 UTC");
    } else {
        panic!("expected Item");
    }
}

#[tokio::test]
async fn test_date_blockheight_encoding_errors_without_provider_timestamp() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "expireAt(uint256)": {
                    "intent": "Expire",
                    "fields": [
                        {"path": "@.0", "label": "Block", "format": "date", "params": {"encoding": "blockheight"}}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata("expireAt(uint256)", &[uint_word(19500000)]);

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let err = format_calldata(&descriptors, &tx, &BlockTimestampProvider(None))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("approximate timestamp"));
}

// ─── #11: domainSeparator parsing ───

#[test]
fn test_domain_separator_parsing() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}],
                "domainSeparator": "0x1234567890abcdef"
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {"definitions": {}, "formats": {}}
    }"#;
    let descriptor = Descriptor::from_json(json).unwrap();
    if let erc7730::types::context::DescriptorContext::Eip712(ctx) = &descriptor.context {
        assert_eq!(
            ctx.eip712.domain_separator.as_deref(),
            Some("0x1234567890abcdef")
        );
    } else {
        panic!("expected Eip712 context");
    }
}

// ─── #12: Encryption fields parsing ───

#[test]
fn test_encryption_full_fields() {
    let json = r#"{
        "path": "secret",
        "label": "Secret",
        "params": {
            "encryption": {
                "scheme": "x25519-xsalsa20-poly1305",
                "plaintextType": "string",
                "fallbackLabel": "Encrypted content"
            }
        }
    }"#;
    let field: erc7730::types::display::DisplayField = serde_json::from_str(json).unwrap();
    if let erc7730::types::display::DisplayField::Simple { params, .. } = &field {
        let enc = params.as_ref().unwrap().encryption.as_ref().unwrap();
        assert_eq!(enc.scheme.as_deref(), Some("x25519-xsalsa20-poly1305"));
        assert_eq!(enc.plaintext_type.as_deref(), Some("string"));
        assert_eq!(enc.fallback_label.as_deref(), Some("Encrypted content"));
    } else {
        panic!("expected Simple");
    }
}

// ─── #10: Factory context parsing ───

#[test]
fn test_factory_context_parsing() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}],
                "factory": {
                    "deployEvent": "ContractCreated(address)",
                    "deployments": [{"chainId": 1, "address": "0xfactory"}]
                }
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {"definitions": {}, "formats": {}}
    }"#;
    let descriptor = Descriptor::from_json(json).unwrap();
    if let erc7730::types::context::DescriptorContext::Contract(ctx) = &descriptor.context {
        let factory = ctx.contract.factory.as_ref().unwrap();
        assert_eq!(
            factory.deploy_event.as_deref(),
            Some("ContractCreated(address)")
        );
        assert_eq!(factory.deployments.len(), 1);
        assert_eq!(factory.deployments[0].address, "0xfactory");
    } else {
        panic!("expected Contract context");
    }
}

// ─── #13: Array slice syntax ───

#[test]
fn test_eip712_array_slice_syntax() {
    let message = serde_json::json!({
        "items": ["a", "b", "c", "d", "e"]
    });

    // Test the resolve_typed_path function indirectly via TypedData
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "Test": {
                    "intent": "Test",
                    "fields": [
                        {"path": "items[1:3]", "label": "Slice"}
                    ]
                }
            }
        }
    }"#;
    let _descriptor = Descriptor::from_json(json).unwrap();
    // The path parsing is tested through integration — here we just verify it parses
    let _ = message;
}

// ─── #14: Unit SI prefix ───

#[tokio::test]
async fn test_unit_si_prefix() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "setGas(uint256)": {
                    "intent": "Set gas",
                    "fields": [
                        {"path": "@.0", "label": "Gas", "format": "unit", "params": {"base": "wei", "prefix": true}}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata("setGas(uint256)", &[uint_word(1_500_000)]);

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
        assert_eq!(item.value, "1.5Mwei");
    } else {
        panic!("expected Item");
    }
}

// ─── #15: Maps keyPath ───

#[tokio::test]
async fn test_maps_key_path() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {
            "owner": "test",
            "enums": {},
            "constants": {},
            "maps": {
                "orderTypes": {
                    "keyPath": "@.0",
                    "entries": {"0": "Market", "1": "Limit", "2": "Stop"}
                }
            }
        },
        "display": {
            "definitions": {},
            "formats": {
                "placeOrder(uint256,uint256)": {
                    "intent": "Place order",
                    "fields": [
                        {"path": "@.1", "label": "Order Type", "params": {"mapReference": "orderTypes"}}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    // arg 0 = 1 (the key), arg 1 = 999 (the field value, not used as key)
    let calldata = build_calldata(
        "placeOrder(uint256,uint256)",
        &[uint_word(1), uint_word(999)],
    );

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
async fn test_eip712_maps_key_path_matches_calldata() {
    let metadata = serde_json::json!({
        "owner": "test",
        "enums": {},
        "constants": {},
        "maps": {
            "orderTypes": {
                "keyPath": "kind",
                "entries": {"0": "Market", "1": "Limit", "2": "Stop"}
            }
        }
    });
    let fields = serde_json::json!([
        {"path": "value", "label": "Order Type", "params": {"mapReference": "orderTypes"}}
    ]);

    let calldata_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "contract": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": metadata.clone(),
            "display": {
                "definitions": {},
                "formats": {
                    "placeOrder(uint256 kind,uint256 value)": {
                        "intent": "Place order",
                        "fields": fields.clone()
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let typed_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": metadata,
            "display": {
                "definitions": {},
                "formats": {
                    "PlaceOrder(uint256 kind,uint256 value)": {
                        "intent": "Place order",
                        "fields": fields
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let calldata = build_calldata(
        "placeOrder(uint256,uint256)",
        &[uint_word(1), uint_word(999)],
    );
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let calldata_result = format_calldata(
        &wrap_rd(calldata_descriptor, 1, "0xabc"),
        &tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "PlaceOrder": [
                { "name": "kind", "type": "uint256" },
                { "name": "value", "type": "uint256" }
            ]
        },
        "primaryType": "PlaceOrder",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "kind": 1, "value": 999 }
    }))
    .unwrap();
    let typed_result = format_typed_data(
        &wrap_rd(typed_descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    assert_semantic_parity(&calldata_result, &typed_result);
}

// ─── #19: Intent as object ───

#[test]
fn test_intent_as_object() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "transfer(address,uint256)": {
                    "intent": {"Action": "Transfer tokens", "Asset": "USDC"},
                    "fields": []
                }
            }
        }
    }"#;
    let descriptor = Descriptor::from_json(json).unwrap();
    let format = descriptor
        .display
        .formats
        .get("transfer(address,uint256)")
        .unwrap();
    let intent_str = erc7730::types::display::intent_as_string(format.intent.as_ref().unwrap());
    assert_eq!(intent_str, "Action: Transfer tokens, Asset: USDC");
}

#[test]
fn test_invalid_nested_intent_object_is_rejected() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "transfer(address,uint256)": {
                    "intent": {"Action": {"nested": "bad"}},
                    "fields": []
                }
            }
        }
    }"#;

    assert!(Descriptor::from_json(json).is_err());
}

#[tokio::test]
async fn test_direct_group_without_label_flattens_into_parent_entries() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "foo(address,uint256)": {
                    "intent": "Grouped",
                    "fields": [{
                        "fields": [
                            {"path": "@.0", "label": "Recipient", "format": "address"},
                            {"path": "@.1", "label": "Amount", "format": "number"}
                        ]
                    }]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata(
        "foo(address,uint256)",
        &[
            addr_word("0x0000000000000000000000000000000000000001"),
            uint_word(100),
        ],
    );

    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
        .await
        .unwrap();

    assert_eq!(result.entries.len(), 2);
    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Recipient"),
        _ => panic!("expected flattened item"),
    }
}

#[tokio::test]
async fn test_calldata_bundled_group_zips_array_items() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "batch(address[] recipients,uint256[] amounts)": {
                    "intent": "Batch",
                    "fields": [{
                        "label": "Transfers",
                        "iteration": "bundled",
                        "fields": [
                            {"path": "recipients.[]", "label": "Recipient", "format": "address"},
                            {"path": "amounts.[]", "label": "Amount", "format": "number"}
                        ]
                    }]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_two_array_calldata(
        "batch(address[],uint256[])",
        &[
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
        ],
        &[100, 200],
    );
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let result = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
        .await
        .unwrap();
    match &result.entries[0] {
        DisplayEntry::Group {
            label,
            iteration,
            items,
        } => {
            assert_eq!(label, "Transfers");
            assert!(matches!(iteration, GroupIteration::Bundled));
            assert_eq!(items.len(), 4);
            assert_eq!(items[0].label, "Recipient");
            assert_eq!(items[1].label, "Amount");
            assert_eq!(items[2].label, "Recipient");
            assert_eq!(items[3].label, "Amount");
        }
        _ => panic!("expected bundled group"),
    }
}

#[tokio::test]
async fn test_eip712_bundled_group_zips_array_items() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Batch(address[] recipients,uint256[] amounts)": {
                        "intent": "Batch",
                        "fields": [{
                            "label": "Transfers",
                            "iteration": "bundled",
                            "fields": [
                                { "path": "recipients.[]", "label": "Recipient", "format": "address" },
                                { "path": "amounts.[]", "label": "Amount", "format": "number" }
                            ]
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Batch": [
                { "name": "recipients", "type": "address[]" },
                { "name": "amounts", "type": "uint256[]" }
            ]
        },
        "primaryType": "Batch",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": {
            "recipients": [
                "0x0000000000000000000000000000000000000001",
                "0x0000000000000000000000000000000000000002"
            ],
            "amounts": [100, 200]
        }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    match &result.entries[0] {
        DisplayEntry::Group {
            iteration, items, ..
        } => {
            assert!(matches!(iteration, GroupIteration::Bundled));
            assert_eq!(items.len(), 4);
        }
        _ => panic!("expected bundled group"),
    }
}

#[tokio::test]
async fn test_eip712_bundled_group_mixed_scalar_child_errors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Batch(address[] recipients,uint256 deadline)": {
                        "intent": "Batch",
                        "fields": [{
                            "iteration": "bundled",
                            "fields": [
                                { "path": "recipients.[]", "label": "Recipient", "format": "address" },
                                { "path": "deadline", "label": "Deadline", "format": "number" }
                            ]
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Batch": [
                { "name": "recipients", "type": "address[]" },
                { "name": "deadline", "type": "uint256" }
            ]
        },
        "primaryType": "Batch",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": {
            "recipients": ["0x0000000000000000000000000000000000000001"],
            "deadline": 123
        }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("bundled groups cannot mix"));
}

// ─── #20: EIP-712 domain completeness ───

#[test]
fn test_eip712_domain_full_fields() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}],
                "domain": {
                    "name": "My App",
                    "version": "2",
                    "chainId": 1,
                    "verifyingContract": "0xabc",
                    "salt": "0xdeadbeef"
                }
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {"definitions": {}, "formats": {}}
    }"#;
    let descriptor = Descriptor::from_json(json).unwrap();
    if let erc7730::types::context::DescriptorContext::Eip712(ctx) = &descriptor.context {
        let domain = ctx.eip712.domain.as_ref().unwrap();
        assert_eq!(domain.name.as_deref(), Some("My App"));
        assert_eq!(domain.version.as_deref(), Some("2"));
        assert_eq!(domain.chain_id, Some(1));
        assert_eq!(domain.verifying_contract.as_deref(), Some("0xabc"));
        assert_eq!(domain.salt.as_deref(), Some("0xdeadbeef"));
    } else {
        panic!("expected Eip712 context");
    }
}

// ─── #22: Escape sequences in interpolation ───

#[tokio::test]
async fn test_interpolation_escape_sequences() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "foo(uint256)": {
                    "intent": "Test",
                    "interpolatedIntent": "Value is {{literal}} and ${@.0}",
                    "fields": [
                        {"path": "@.0", "label": "Val", "format": "number"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata("foo(uint256)", &[uint_word(42)]);

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
    assert_eq!(
        result.interpolated_intent.as_deref(),
        Some("Value is {literal} and 42")
    );
}

// ─── #16: EIP-712 AddressName with senderAddress ───

#[tokio::test]
async fn test_eip712_address_name_sender() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "Transfer(address to)": {
                    "intent": "Transfer",
                    "fields": [
                        {
                            "path": "to",
                            "label": "Recipient",
                            "format": "addressName",
                            "params": {
                                "senderAddress": "0x1234567890123456789012345678901234567890"
                            }
                        }
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {"EIP712Domain": [], "Transfer": [{"name": "to", "type": "address"}]},
        "primaryType": "Transfer",
        "domain": {"chainId": 1, "verifyingContract": "0xabc"},
        "message": {"to": "0x1234567890123456789012345678901234567890"}
    }))
    .unwrap();

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.value, "Sender");
    } else {
        panic!("expected Item");
    }
}

// ─── #8: selectorPath parsing ───

#[test]
fn test_selector_path_parsing() {
    let json = r#"{
        "path": "data",
        "label": "Inner call",
        "format": "calldata",
        "params": {
            "calleePath": "to",
            "selectorPath": "selector"
        }
    }"#;
    let field: erc7730::types::display::DisplayField = serde_json::from_str(json).unwrap();
    if let erc7730::types::display::DisplayField::Simple { params, .. } = &field {
        let p = params.as_ref().unwrap();
        assert_eq!(p.selector_path.as_deref(), Some("selector"));
        assert_eq!(p.callee_path.as_deref(), Some("to"));
    } else {
        panic!("expected Simple");
    }
}

// ─── #2: EIP-712 with literal value field ───

#[tokio::test]
async fn test_eip712_literal_value_field() {
    let json = r#"{
        "context": {
            "eip712": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "Permit(address spender)": {
                    "intent": "Permit",
                    "fields": [
                        {"value": "Token Approval", "label": "Action"},
                        {"path": "spender", "label": "Spender", "format": "address"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {"EIP712Domain": [], "Permit": [{"name": "spender", "type": "address"}]},
        "primaryType": "Permit",
        "domain": {"chainId": 1, "verifyingContract": "0xabc"},
        "message": {"spender": "0x1234567890123456789012345678901234567890"}
    }))
    .unwrap();

    let descriptors = wrap_rd(descriptor, 1, "0xabc");
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();
    assert_eq!(result.entries.len(), 2);
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Action");
        assert_eq!(item.value, "Token Approval");
    } else {
        panic!("expected Item");
    }
}

// ─── #21: Excluded paths ───

#[tokio::test]
async fn test_excluded_paths() {
    let json = r#"{
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "foo(uint256,uint256)": {
                    "intent": "Test excluded",
                    "excluded": ["@.1"],
                    "fields": [
                        {"path": "@.0", "label": "Visible", "format": "number"},
                        {"path": "@.1", "label": "Excluded", "format": "number"}
                    ]
                }
            }
        }
    }"#;

    let descriptor = Descriptor::from_json(json).unwrap();
    let calldata = build_calldata("foo(uint256,uint256)", &[uint_word(42), uint_word(99)]);

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
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Visible");
        assert_eq!(item.value, "42");
    } else {
        panic!("expected Item");
    }
}

#[tokio::test]
async fn test_eip712_excluded_paths_match_calldata() {
    let fields = serde_json::json!([
        {"path": "visible", "label": "Visible", "format": "number"},
        {"path": "hidden", "label": "Hidden", "format": "number"}
    ]);

    let calldata_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "contract": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "show(uint256 visible,uint256 hidden)": {
                        "intent": "Show",
                        "excluded": ["hidden"],
                        "fields": fields.clone()
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let typed_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "Show(uint256 visible,uint256 hidden)": {
                        "intent": "Show",
                        "excluded": ["hidden"],
                        "fields": fields
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let calldata = build_calldata("show(uint256,uint256)", &[uint_word(42), uint_word(99)]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let calldata_result = format_calldata(
        &wrap_rd(calldata_descriptor, 1, "0xabc"),
        &tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Show": [
                { "name": "visible", "type": "uint256" },
                { "name": "hidden", "type": "uint256" }
            ]
        },
        "primaryType": "Show",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "visible": 42, "hidden": 99 }
    }))
    .unwrap();
    let typed_result = format_typed_data(
        &wrap_rd(typed_descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    assert_semantic_parity(&calldata_result, &typed_result);
}

#[tokio::test]
async fn test_eip712_token_amount_threshold_matches_calldata() {
    let token = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let fields = serde_json::json!([
        {
            "path": "value",
            "label": "Amount",
            "format": "tokenAmount",
            "params": {
                "token": token,
                "threshold": "0x100",
                "message": "All"
            }
        }
    ]);

    let calldata_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "contract": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "withdraw(uint256 value)": {
                        "intent": "Withdraw",
                        "fields": fields.clone()
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let typed_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "Withdraw(uint256 value)": {
                        "intent": "Withdraw",
                        "fields": fields
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let calldata = build_calldata("withdraw(uint256)", &[uint_word(256)]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        1,
        token,
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );

    let calldata_result = format_calldata(&wrap_rd(calldata_descriptor, 1, "0xabc"), &tx, &tokens)
        .await
        .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Withdraw": [{ "name": "value", "type": "uint256" }]
        },
        "primaryType": "Withdraw",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "value": "256" }
    }))
    .unwrap();
    let typed_result =
        format_typed_data(&wrap_rd(typed_descriptor, 1, "0xabc"), &typed_data, &tokens)
            .await
            .unwrap();

    assert_semantic_parity(&calldata_result, &typed_result);
}

#[tokio::test]
async fn test_eip712_token_amount_native_currency_matches_calldata() {
    let native = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let fields = serde_json::json!([
        {
            "path": "value",
            "label": "Amount",
            "format": "tokenAmount",
            "params": {
                "tokenPath": "token",
                "nativeCurrencyAddress": native
            }
        }
    ]);

    let calldata_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "contract": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "swap(address token,uint256 value)": {
                        "intent": "Swap",
                        "fields": fields.clone()
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let typed_descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}]
                }
            },
            "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
            "display": {
                "definitions": {},
                "formats": {
                    "Swap(address token,uint256 value)": {
                        "intent": "Swap",
                        "fields": fields
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let calldata = build_calldata(
        "swap(address,uint256)",
        &[addr_word(native), uint_word(1_000_000_000_000_000_000)],
    );
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let calldata_result = format_calldata(
        &wrap_rd(calldata_descriptor, 1, "0xabc"),
        &tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Swap": [
                { "name": "token", "type": "address" },
                { "name": "value", "type": "uint256" }
            ]
        },
        "primaryType": "Swap",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": {
            "token": native,
            "value": "1000000000000000000"
        }
    }))
    .unwrap();
    let typed_result = format_typed_data(
        &wrap_rd(typed_descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    assert_semantic_parity(&calldata_result, &typed_result);
}

// ─── #17: Includes mechanism ───

#[test]
fn test_merge_fields_by_path() {
    let included = serde_json::json!({
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "generic", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {
                "approve(address spender,uint256 value)": {
                    "intent": "Approve",
                    "fields": [
                        {"path": "spender", "label": "Spender", "format": "addressName"},
                        {"path": "value", "label": "Amount", "format": "tokenAmount",
                         "params": {"tokenPath": "@.to", "threshold": "0x800"}}
                    ]
                }
            }
        }
    });

    let including = serde_json::json!({
        "includes": "./erc20.json",
        "display": {
            "formats": {
                "approve(address spender,uint256 value)": {
                    "fields": [
                        {"path": "value", "params": {"threshold": "0xFFF"}}
                    ]
                }
            }
        }
    });

    let merged = merge_descriptor_values(&including, &included);
    let fields = merged["display"]["formats"]["approve(address spender,uint256 value)"]["fields"]
        .as_array()
        .unwrap();
    assert_eq!(fields.len(), 2);
    // Spender field preserved from included
    assert_eq!(fields[0]["path"], "spender");
    assert_eq!(fields[0]["label"], "Spender");
    // Amount field: threshold overridden, tokenPath preserved
    assert_eq!(fields[1]["path"], "value");
    assert_eq!(fields[1]["label"], "Amount");
    assert_eq!(fields[1]["params"]["threshold"], "0xFFF");
    assert_eq!(fields[1]["params"]["tokenPath"], "@.to");
}

#[test]
fn test_merge_including_wins_metadata() {
    let included = serde_json::json!({
        "metadata": {"owner": "Generic", "contractName": "ERC20"}
    });
    let including = serde_json::json!({
        "metadata": {"owner": "Tether", "contractName": "USDT"}
    });
    let merged = merge_descriptor_values(&including, &included);
    assert_eq!(merged["metadata"]["owner"], "Tether");
    assert_eq!(merged["metadata"]["contractName"], "USDT");
}

#[test]
fn test_merge_format_keys() {
    let included = serde_json::json!({
        "display": {
            "definitions": {},
            "formats": {
                "transfer(address,uint256)": {
                    "intent": "Transfer",
                    "fields": [{"path": "@.0", "label": "To"}]
                },
                "approve(address,uint256)": {
                    "intent": "Approve",
                    "fields": [{"path": "@.0", "label": "Spender"}]
                }
            }
        }
    });
    let including = serde_json::json!({
        "display": {
            "formats": {
                "transfer(address,uint256)": {
                    "intent": "Send tokens"
                }
            }
        }
    });
    let merged = merge_descriptor_values(&including, &included);
    // transfer intent overridden
    assert_eq!(
        merged["display"]["formats"]["transfer(address,uint256)"]["intent"],
        "Send tokens"
    );
    // transfer fields preserved from base
    assert!(
        merged["display"]["formats"]["transfer(address,uint256)"]["fields"]
            .as_array()
            .unwrap()
            .len()
            == 1
    );
    // approve format preserved from base
    assert_eq!(
        merged["display"]["formats"]["approve(address,uint256)"]["intent"],
        "Approve"
    );
}

#[test]
fn test_merge_appends_new_fields() {
    let included = serde_json::json!({
        "display": {
            "definitions": {},
            "formats": {
                "foo(uint256)": {
                    "intent": "Foo",
                    "fields": [{"path": "@.0", "label": "Existing"}]
                }
            }
        }
    });
    let including = serde_json::json!({
        "display": {
            "formats": {
                "foo(uint256)": {
                    "fields": [{"path": "@.1", "label": "New"}]
                }
            }
        }
    });
    let merged = merge_descriptor_values(&including, &included);
    let fields = merged["display"]["formats"]["foo(uint256)"]["fields"]
        .as_array()
        .unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0]["path"], "@.0");
    assert_eq!(fields[1]["path"], "@.1");
}

#[test]
fn test_merge_context_from_including() {
    let included = serde_json::json!({
        "context": {
            "contract": {"abi": ["function transfer(address,uint256)"]}
        }
    });
    let including = serde_json::json!({
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xdAC17"}]
            }
        }
    });
    let merged = merge_descriptor_values(&including, &included);
    // Both abi and deployments present via deep merge
    assert!(merged["context"]["contract"]["abi"].is_array());
    assert!(merged["context"]["contract"]["deployments"].is_array());
}

#[test]
fn test_merge_preserves_included_fields() {
    let included = serde_json::json!({
        "display": {
            "definitions": {},
            "formats": {
                "foo(address,uint256)": {
                    "intent": "Foo",
                    "fields": [
                        {"path": "@.0", "label": "Recipient", "format": "address"},
                        {"path": "@.1", "label": "Amount", "format": "number"}
                    ]
                }
            }
        }
    });
    // Including file doesn't touch these fields at all
    let including = serde_json::json!({
        "metadata": {"owner": "Override"}
    });
    let merged = merge_descriptor_values(&including, &included);
    let fields = merged["display"]["formats"]["foo(address,uint256)"]["fields"]
        .as_array()
        .unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0]["label"], "Recipient");
    assert_eq!(fields[1]["label"], "Amount");
}

#[test]
fn test_merge_nested_params() {
    let included = serde_json::json!({
        "display": {
            "definitions": {},
            "formats": {
                "foo(uint256)": {
                    "intent": "Foo",
                    "fields": [{
                        "path": "@.0", "label": "Amount", "format": "tokenAmount",
                        "params": {"tokenPath": "@.to", "threshold": "0x100", "nativeCurrencyAddress": "0xEEE"}
                    }]
                }
            }
        }
    });
    let including = serde_json::json!({
        "display": {
            "formats": {
                "foo(uint256)": {
                    "fields": [{
                        "path": "@.0",
                        "params": {"threshold": "0xFFF"}
                    }]
                }
            }
        }
    });
    let merged = merge_descriptor_values(&including, &included);
    let field = &merged["display"]["formats"]["foo(uint256)"]["fields"][0];
    assert_eq!(field["params"]["threshold"], "0xFFF");
    assert_eq!(field["params"]["tokenPath"], "@.to");
    assert_eq!(field["params"]["nativeCurrencyAddress"], "0xEEE");
}

#[test]
fn test_includes_deserialization() {
    let json = r#"{
        "includes": "./base.json",
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xabc"}]
            }
        },
        "metadata": {"owner": "test", "enums": {}, "constants": {}, "maps": {}},
        "display": {
            "definitions": {},
            "formats": {}
        }
    }"#;
    let descriptor = Descriptor::from_json(json).unwrap();
    assert_eq!(descriptor.includes.as_deref(), Some("./base.json"));
}

#[test]
fn test_merge_strips_includes() {
    let including = serde_json::json!({
        "includes": "./base.json",
        "metadata": {"owner": "Override"}
    });
    let included = serde_json::json!({
        "metadata": {"owner": "Base"}
    });
    let merged = merge_descriptor_values(&including, &included);
    assert!(merged.get("includes").is_none());
}

#[tokio::test]
async fn test_merge_produces_valid_descriptor() {
    // Full end-to-end: merge two partial descriptors, then use the result for formatting
    let included_json = r#"{
        "display": {
            "definitions": {},
            "formats": {
                "transfer(address to,uint256 amount)": {
                    "intent": "Transfer",
                    "fields": [
                        {"path": "to", "label": "Recipient", "format": "address"},
                        {"path": "amount", "label": "Amount", "format": "number"}
                    ]
                }
            }
        }
    }"#;

    let including_json = r#"{
        "includes": "./erc20.json",
        "context": {
            "contract": {
                "deployments": [{"chainId": 1, "address": "0xdac17f958d2ee523a2206206994597c13d831ec7"}]
            }
        },
        "metadata": {"owner": "Tether", "contractName": "USDT", "enums": {}, "constants": {}, "maps": {}}
    }"#;

    let merged_json = merge_descriptors(including_json, included_json).unwrap();
    let descriptor = Descriptor::from_json(&merged_json).unwrap();

    let calldata = build_calldata(
        "transfer(address,uint256)",
        &[
            addr_word("0x0000000000000000000000000000000000000001"),
            uint_word(1000),
        ],
    );

    let descriptors = wrap_rd(descriptor, 1, "0xdac17f958d2ee523a2206206994597c13d831ec7");
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xdac17f958d2ee523a2206206994597c13d831ec7",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap();
    assert_eq!(result.intent, "Transfer");
    assert_eq!(result.entries.len(), 2);
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Recipient");
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

// ─── EIP-712 encodeType format key matching ───

#[tokio::test]
async fn test_eip712_encode_type_format_key() {
    // Real Velora/Portikus DeltaV2 descriptor — format key is the full encodeType string
    let descriptor_json = r#"{
        "context": {
            "eip712": {
                "deployments": [
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
                        { "path": "bridge.destinationChainId", "label": "Destination chain ID", "format": "raw" },
                        { "path": "beneficiary", "label": "Beneficiary", "format": "raw" },
                        { "path": "deadline", "label": "Expiration time", "format": "date", "params": { "encoding": "timestamp" } }
                    ]
                }
            }
        }
    }"#;

    // Real typed data from wallet — primaryType is "Order", not the full encodeType key
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
            "destAmount": "18816200237962656",
            "expectedAmount": "18910754008002670",
            "deadline": 1774257465,
            "nonce": "1774257068031",
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

    let descriptor = Descriptor::from_json(descriptor_json).unwrap();
    let typed_data: TypedData = serde_json::from_str(typed_data_json).unwrap();
    let descriptors = wrap_rd(descriptor, 10, "0x0000000000bbf5c5fd284e657f01bd000933c96d");

    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    // Must match the descriptor format, not fall back to raw
    assert_eq!(result.intent, "Swap order");
    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(result.entries.len(), 5);

    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Amount to send");
    } else {
        panic!("expected Item for Amount to send");
    }
    if let DisplayEntry::Item(ref item) = result.entries[1] {
        assert_eq!(item.label, "Minimum to receive");
    } else {
        panic!("expected Item for Minimum to receive");
    }
    if let DisplayEntry::Item(ref item) = result.entries[2] {
        assert_eq!(item.label, "Destination chain ID");
        assert_eq!(item.value, "0");
    } else {
        panic!("expected Item for Destination chain ID");
    }
    if let DisplayEntry::Item(ref item) = result.entries[3] {
        assert_eq!(item.label, "Beneficiary");
        assert_eq!(item.value, "0xbf01daf454dce008d3e2bfd47d5e186f71477253");
    } else {
        panic!("expected Item for Beneficiary");
    }
    if let DisplayEntry::Item(ref item) = result.entries[4] {
        assert_eq!(item.label, "Expiration time");
    } else {
        panic!("expected Item for Expiration time");
    }
}

#[tokio::test]
async fn test_eip712_bare_primary_type_key_falls_back_for_legacy_descriptor() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert_eq!(result.intent, "Permit");
}

#[tokio::test]
async fn test_eip712_real_world_legacy_receive_with_authorization_key_still_formats() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 42161, "address": "0xaf88d065e77c8cc2239327c5edb3a432268e5831"}] } },
            "metadata": { "owner": "Circle", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "ReceiveWithAuthorization": {
                        "intent": "Authorize USDC transfer",
                        "fields": [
                            { "path": "from", "label": "From", "format": "addressName" },
                            { "path": "to", "label": "To", "format": "addressName" },
                            { "path": "value", "label": "Amount", "format": "tokenAmount", "params": { "tokenPath": "@.to" } }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "ReceiveWithAuthorization": [
                { "name": "from", "type": "address" },
                { "name": "to", "type": "address" },
                { "name": "value", "type": "uint256" },
                { "name": "validAfter", "type": "uint256" },
                { "name": "validBefore", "type": "uint256" },
                { "name": "nonce", "type": "bytes32" }
            ]
        },
        "primaryType": "ReceiveWithAuthorization",
        "domain": { "chainId": 42161, "verifyingContract": "0xaf88d065e77c8cc2239327c5edb3a432268e5831" },
        "message": {
            "from": "0xbf01daf454dce008d3e2bfd47d5e186f71477253",
            "to": "0xaf88d065e77c8cc2239327c5edb3a432268e5831",
            "value": "6050000",
            "validAfter": 1774607678,
            "validBefore": 1774611338,
            "nonce": "0x073a2d085bbb11c3d51a9ce8ed3105ed0892dbaa516b8f2d2853fd4d6e0054d4"
        }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(
            descriptor,
            42161,
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831",
        ),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert_eq!(result.intent, "Authorize USDC transfer");
}

#[tokio::test]
async fn test_eip712_prefix_only_format_key_rejected() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender,uint256 extra)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("no EIP-712 display format found"));
}

#[tokio::test]
async fn test_eip712_canonical_key_wins_over_legacy_primary_type_key() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit": {
                        "intent": "Legacy Permit",
                        "fields": [{ "path": "spender", "label": "Legacy", "format": "address" }]
                    },
                    "Permit(address spender)": {
                        "intent": "Canonical Permit",
                        "fields": [{ "path": "spender", "label": "Canonical", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert_eq!(result.intent, "Canonical Permit");
    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Canonical"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_eip712_missing_chain_id_rejected_with_descriptors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": { "verifyingContract": "0xabc" },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("domain.chainId is required"));
}

#[tokio::test]
async fn test_eip712_missing_verifying_contract_rejected_with_descriptors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": { "chainId": 1 },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("domain.verifyingContract is required"));
}

#[tokio::test]
async fn test_eip712_outer_descriptor_match_is_required() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xdef"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xdef"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("no EIP-712 descriptor found"));
}

#[tokio::test]
async fn test_eip712_descriptor_domain_name_match_succeeds() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "Permit2" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "name": "Permit2",
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Spender"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_eip712_descriptor_domain_name_mismatch_rejects() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "Permit2" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "name": "Other",
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("descriptor eip712.domain.name mismatch"));
}

#[tokio::test]
async fn test_eip712_descriptor_domain_version_mismatch_rejects() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "version": "1" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "version": "2",
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("descriptor eip712.domain.version mismatch"));
}

#[tokio::test]
async fn test_eip712_descriptor_domain_chain_id_mismatch_rejects_after_deployment_match() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "chainId": 10 }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("descriptor eip712.domain.chainId mismatch"));
}

#[tokio::test]
async fn test_eip712_descriptor_domain_verifying_contract_match_is_case_insensitive() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xaBc"}],
                    "domain": { "verifyingContract": "0xaBc" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "chainId": 1,
            "verifyingContract": "0xAbC"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xaBc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Spender"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_eip712_descriptor_domain_salt_match_succeeds() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "salt": "0xdeadbeef" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "chainId": 1,
            "verifyingContract": "0xabc",
            "salt": "0Xdeadbeef"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Spender"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_eip712_descriptor_domain_salt_mismatch_rejects() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "salt": "0xdeadbeef" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "chainId": 1,
            "verifyingContract": "0xabc",
            "salt": "0xbeefdead"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("descriptor eip712.domain.salt mismatch"));
}

#[tokio::test]
async fn test_eip712_descriptor_domain_salt_missing_rejects() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "salt": "0xdeadbeef" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains(
        "descriptor eip712.domain.salt is required by descriptor but missing from typed data"
    ));
}

#[tokio::test]
async fn test_eip712_descriptor_domain_omitted_field_is_not_enforced() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "Permit2" }
                }
            },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit",
                        "fields": [{ "path": "spender", "label": "Spender", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "name": "Permit2",
            "version": "7",
            "chainId": 1,
            "verifyingContract": "0xabc",
            "salt": "0x1234"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Spender"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_eip712_domain_binding_selects_correct_descriptor() {
    let descriptor_a = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "Permit2" }
                }
            },
            "metadata": { "owner": "a", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit A",
                        "fields": [{ "path": "spender", "label": "Spender A", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let descriptor_b = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "AllowanceTransfer" }
                }
            },
            "metadata": { "owner": "b", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(address spender)": {
                        "intent": "Permit B",
                        "fields": [{ "path": "spender", "label": "Spender B", "format": "address" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "name": "AllowanceTransfer",
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let descriptors = vec![
        wrap_rd(descriptor_a, 1, "0xabc")
            .into_iter()
            .next()
            .unwrap(),
        wrap_rd(descriptor_b, 1, "0xabc")
            .into_iter()
            .next()
            .unwrap(),
    ];

    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Spender B"),
        _ => panic!("expected Item"),
    }
    assert_eq!(result.owner.as_deref(), Some("b"));
}

#[tokio::test]
async fn test_eip712_domain_binding_ambiguity_errors() {
    let descriptor_a = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "Permit2" }
                }
            },
            "metadata": { "owner": "a", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit A", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let descriptor_b = Descriptor::from_json(
        r#"{
            "context": {
                "eip712": {
                    "deployments": [{"chainId": 1, "address": "0xabc"}],
                    "domain": { "name": "Permit2" }
                }
            },
            "metadata": { "owner": "b", "enums": {}, "constants": {}, "maps": {} },
            "display": { "definitions": {}, "formats": { "Permit(address spender)": { "intent": "Permit B", "fields": [] } } }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Permit": [{ "name": "spender", "type": "address" }] },
        "primaryType": "Permit",
        "domain": {
            "name": "Permit2",
            "chainId": 1,
            "verifyingContract": "0xabc"
        },
        "message": { "spender": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let descriptors = vec![
        wrap_rd(descriptor_a, 1, "0xabc")
            .into_iter()
            .next()
            .unwrap(),
        wrap_rd(descriptor_b, 1, "0xabc")
            .into_iter()
            .next()
            .unwrap(),
    ];

    let err = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("multiple EIP-712 descriptors match"));
}

#[tokio::test]
async fn test_eip712_sender_address_uses_container_from() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Transfer(address to)": {
                        "intent": "Transfer",
                        "fields": [{
                            "path": "to",
                            "label": "Recipient",
                            "format": "addressName",
                            "params": { "senderAddress": "@.from" }
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Transfer": [{ "name": "to", "type": "address" }] },
        "primaryType": "Transfer",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "container": { "from": "0x1234567890123456789012345678901234567890" },
        "message": { "to": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.value, "Sender"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_eip712_sender_address_missing_container_from_errors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Transfer(address to)": {
                        "intent": "Transfer",
                        "fields": [{
                            "path": "to",
                            "label": "Recipient",
                            "format": "addressName",
                            "params": { "senderAddress": "@.from" }
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "Transfer": [{ "name": "to", "type": "address" }] },
        "primaryType": "Transfer",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "to": "0x1234567890123456789012345678901234567890" }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("@.from is required"));
}

#[tokio::test]
async fn test_calldata_interpolation_placeholder_without_field_spec_errors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "foo(uint256)": {
                        "intent": "Foo",
                        "interpolatedIntent": "Missing {missing}",
                        "fields": [{ "path": "@.0", "label": "Value", "format": "number" }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let calldata = build_calldata("foo(uint256)", &[uint_word(42)]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let err = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not match any display field"));
}

#[tokio::test]
async fn test_eip712_interpolation_placeholder_for_calldata_field_errors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Relay(address to,bytes data)": {
                        "intent": "Relay",
                        "interpolatedIntent": "Relay {data}",
                        "fields": [
                            { "path": "to", "label": "To", "visible": "never" },
                            { "path": "data", "label": "Call", "format": "calldata", "params": { "calleePath": "to" } }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Relay": [
                { "name": "to", "type": "address" },
                { "name": "data", "type": "bytes" }
            ]
        },
        "primaryType": "Relay",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": {
            "to": "0x1234567890123456789012345678901234567890",
            "data": "0x12345678"
        }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("non-stringable calldata field"));
}

#[tokio::test]
async fn test_eip712_group_only_interpolation_path_errors() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Quote(Details details)Details(uint256 amount)": {
                        "intent": "Quote",
                        "interpolatedIntent": "Quote {details}",
                        "fields": [{
                            "path": "details",
                            "fields": [
                                { "path": "amount", "label": "Amount", "format": "number" }
                            ]
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Quote": [{ "name": "details", "type": "Details" }],
            "Details": [{ "name": "amount", "type": "uint256" }]
        },
        "primaryType": "Quote",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "details": { "amount": 1250 } }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("does not match any display field"));
}

#[tokio::test]
async fn test_eip712_scoped_field_interpolation_matches_rendering() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Quote(Details details)Details(uint256 amount)": {
                        "intent": "Quote",
                        "interpolatedIntent": "Quote {details.amount}",
                        "fields": [{
                            "path": "details",
                            "fields": [
                                { "path": "amount", "label": "Amount", "format": "unit", "params": { "base": "%", "decimals": 2 } }
                            ]
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Quote": [{ "name": "details", "type": "Details" }],
            "Details": [{ "name": "amount", "type": "uint256" }]
        },
        "primaryType": "Quote",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "details": { "amount": 1250 } }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.value, "12.5%"),
        _ => panic!("expected Item"),
    }
    assert_eq!(result.interpolated_intent.as_deref(), Some("Quote 12.5%"));
}

#[tokio::test]
async fn test_eip712_ref_field_interpolation_matches_rendering() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {
                    "rateField": {
                        "label": "Rate",
                        "format": "unit",
                        "params": { "base": "%", "decimals": 2 }
                    }
                },
                "formats": {
                    "SetRate(uint256 rate)": {
                        "intent": "Set rate",
                        "interpolatedIntent": "Rate {rate}",
                        "fields": [
                            { "$ref": "$.display.definitions.rateField", "path": "rate" }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": { "EIP712Domain": [], "SetRate": [{ "name": "rate", "type": "uint256" }] },
        "primaryType": "SetRate",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "rate": 1250 }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.value, "12.5%"),
        _ => panic!("expected Item"),
    }
    assert_eq!(result.interpolated_intent.as_deref(), Some("Rate 12.5%"));
}

#[tokio::test]
async fn test_eip712_interpolation_uses_same_formatting_as_fields() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": {
                "owner": "test",
                "enums": { "kind": { "2": "Variable" } },
                "constants": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    "Order(address to,uint256 amount,uint256 deadline,uint8 kind)": {
                        "intent": "Order",
                        "interpolatedIntent": "Send {amount} to {to} as {kind} before {deadline}",
                        "fields": [
                            { "path": "to", "label": "To", "format": "addressName", "params": { "senderAddress": "0x1234567890123456789012345678901234567890" } },
                            { "path": "amount", "label": "Amount", "format": "tokenAmount", "params": { "token": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" } },
                            { "path": "kind", "label": "Kind", "format": "enum", "params": { "enumPath": "kind" } },
                            { "path": "deadline", "label": "Deadline", "format": "date", "params": { "encoding": "timestamp" } }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Order": [
                { "name": "to", "type": "address" },
                { "name": "amount", "type": "uint256" },
                { "name": "deadline", "type": "uint256" },
                { "name": "kind", "type": "uint8" }
            ]
        },
        "primaryType": "Order",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": {
            "to": "0x1234567890123456789012345678901234567890",
            "amount": "1500000",
            "deadline": 1700000000,
            "kind": 2
        }
    }))
    .unwrap();

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        1,
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );

    let result = format_typed_data(&wrap_rd(descriptor, 1, "0xabc"), &typed_data, &tokens)
        .await
        .unwrap();
    assert_eq!(
        result.interpolated_intent.as_deref(),
        Some("Send 1.5 USDC to Sender as Variable before 2023-11-14 22:13:20 UTC")
    );
}

#[tokio::test]
async fn test_visible_optional_displays_by_default() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "show(uint256 value)": {
                        "intent": "Show",
                        "fields": [
                            { "path": "value", "label": "Value", "format": "number", "visible": "optional" }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let calldata = build_calldata("show(uint256)", &[uint_word(7)]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let result = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
        .await
        .unwrap();
    assert_eq!(result.entries.len(), 1);
    match &result.entries[0] {
        DisplayEntry::Item(item) => {
            assert_eq!(item.label, "Value");
            assert_eq!(item.value, "7");
        }
        _ => panic!("expected Item"),
    }
}

#[test]
fn test_unknown_visibility_string_is_rejected() {
    assert!(Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "show(uint256 value)": {
                        "intent": "Show",
                        "fields": [
                            { "path": "value", "label": "Value", "visible": "sometimes" }
                        ]
                    }
                }
            }
        }"#,
    )
    .is_err());
}

#[tokio::test]
async fn test_visible_if_not_in_hides_matching_value_and_shows_non_matching() {
    let descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": { "contract": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "show(uint256 value)": {
                        "intent": "Show",
                        "fields": [
                            {
                                "path": "value",
                                "label": "Value",
                                "format": "number",
                                "visible": { "ifNotIn": [uint_hex_literal(0)] }
                            }
                        ]
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let hidden = build_calldata("show(uint256)", &[uint_word(0)]);
    let hidden_tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &hidden,
        value: None,
        from: None,
        implementation_address: None,
    };
    let hidden_result = format_calldata(
        &wrap_rd(descriptor.clone(), 1, "0xabc"),
        &hidden_tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert!(hidden_result.entries.is_empty());

    let shown = build_calldata("show(uint256)", &[uint_word(9)]);
    let shown_tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &shown,
        value: None,
        from: None,
        implementation_address: None,
    };
    let shown_result = format_calldata(
        &wrap_rd(descriptor, 1, "0xabc"),
        &shown_tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert_eq!(shown_result.entries.len(), 1);
}

#[tokio::test]
async fn test_visible_must_match_hides_matching_value_and_errors_on_mismatch() {
    let descriptor = Descriptor::from_json(
        &serde_json::json!({
            "context": { "contract": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "show(uint256 guard,uint256 value)": {
                        "intent": "Show",
                        "fields": [
                            {
                                "path": "guard",
                                "label": "Guard",
                                "format": "number",
                                "visible": { "mustMatch": [uint_hex_literal(1)] }
                            },
                            { "path": "value", "label": "Value", "format": "number" }
                        ]
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let matching = build_calldata("show(uint256,uint256)", &[uint_word(1), uint_word(5)]);
    let matching_tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &matching,
        value: None,
        from: None,
        implementation_address: None,
    };
    let matching_result = format_calldata(
        &wrap_rd(descriptor.clone(), 1, "0xabc"),
        &matching_tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert_eq!(matching_result.entries.len(), 1);
    match &matching_result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Value"),
        _ => panic!("expected Item"),
    }

    let mismatching = build_calldata("show(uint256,uint256)", &[uint_word(2), uint_word(5)]);
    let mismatching_tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &mismatching,
        value: None,
        from: None,
        implementation_address: None,
    };
    let err = format_calldata(
        &wrap_rd(descriptor, 1, "0xabc"),
        &mismatching_tx,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("visible.mustMatch"));
}

#[tokio::test]
async fn test_typed_visibility_alias_must_be_behaves_like_must_match() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(uint256 flag,uint256 value)": {
                        "intent": "Permit",
                        "fields": [
                            { "path": "flag", "label": "Flag", "format": "number", "visible": { "mustBe": [1] } },
                            { "path": "value", "label": "Value", "format": "number" }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Permit": [
                { "name": "flag", "type": "uint256" },
                { "name": "value", "type": "uint256" }
            ]
        },
        "primaryType": "Permit",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "flag": 1, "value": 9 }
    }))
    .unwrap();

    let result = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap();
    assert_eq!(result.entries.len(), 1);
    match &result.entries[0] {
        DisplayEntry::Item(item) => assert_eq!(item.label, "Value"),
        _ => panic!("expected Item"),
    }
}

#[tokio::test]
async fn test_typed_visibility_must_match_errors_when_value_missing() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Permit(uint256 value)": {
                        "intent": "Permit",
                        "fields": [
                            { "path": "missing", "label": "Missing", "format": "number", "visible": { "mustMatch": [1] } }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Permit": [{ "name": "value", "type": "uint256" }]
        },
        "primaryType": "Permit",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "value": 9 }
    }))
    .unwrap();

    let err = format_typed_data(
        &wrap_rd(descriptor, 1, "0xabc"),
        &typed_data,
        &EmptyDataProvider,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("visible.mustMatch"));
}

#[test]
fn test_nested_calldata_constant_params_parse() {
    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "outer(bytes data)": {
                        "intent": "Outer",
                        "fields": [{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {
                                "callee": "0x1000000000000000000000000000000000000001",
                                "selector": "0x12345678",
                                "chainId": 10,
                                "amount": "42",
                                "spender": "0x2000000000000000000000000000000000000002"
                            }
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let field = descriptor.display.formats["outer(bytes data)"].fields[0].clone();
    if let erc7730::types::display::DisplayField::Simple { params, .. } = field {
        let params = params.unwrap();
        assert_eq!(
            params.callee.as_deref(),
            Some("0x1000000000000000000000000000000000000001")
        );
        assert_eq!(params.selector.as_deref(), Some("0x12345678"));
        assert_eq!(params.chain_id, Some(10));
        assert_eq!(
            params.spender.as_deref(),
            Some("0x2000000000000000000000000000000000000002")
        );
        assert_eq!(
            params.amount.unwrap().to_biguint().unwrap().to_string(),
            "42"
        );
    } else {
        panic!("expected Simple field");
    }
}

#[tokio::test]
async fn test_calldata_nested_calldata_constant_params_render() {
    let inner_sig = decoder::parse_signature("consume()").unwrap();
    let inner_selector = format!("0x{}", hex::encode(inner_sig.selector));
    let inner_addr = "0x1000000000000000000000000000000000000001";
    let spender = "0x2000000000000000000000000000000000000002";

    let outer_json = format!(
        r#"{{
            "context": {{ "contract": {{ "deployments": [{{"chainId": 1, "address": "0xabc"}}] }} }},
            "metadata": {{ "owner": "test", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
            "display": {{
                "definitions": {{}},
                "formats": {{
                    "outer(bytes data)": {{
                        "intent": "Outer",
                        "fields": [{{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {{
                                "callee": "{inner_addr}",
                                "selector": "{inner_selector}",
                                "chainId": 10,
                                "amount": "7",
                                "spender": "{spender}"
                            }}
                        }}]
                    }}
                }}
            }}
        }}"#
    );
    let outer = Descriptor::from_json(&outer_json).unwrap();
    let inner = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 10, "address": "0x1000000000000000000000000000000000000001"}] } },
            "metadata": { "owner": "inner", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "consume()": {
                        "intent": "Consume",
                        "fields": [
                            { "path": "@.from", "label": "Spender", "format": "address" },
                            { "path": "@.value", "label": "Amount", "format": "number" }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let calldata = build_single_bytes_calldata("outer(bytes)", &[]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = vec![
        wrap_rd(outer, 1, "0xabc").into_iter().next().unwrap(),
        wrap_rd(inner, 10, inner_addr).into_iter().next().unwrap(),
    ];
    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap();

    match &result.entries[0] {
        DisplayEntry::Nested {
            intent, entries, ..
        } => {
            assert_eq!(intent, "Consume");
            assert!(entries.iter().any(|entry| matches!(
                entry,
                DisplayEntry::Item(item) if item.label == "Spender" && item.value == "0x2000000000000000000000000000000000000002"
            )));
            assert!(entries.iter().any(|entry| matches!(
                entry,
                DisplayEntry::Item(item) if item.label == "Amount" && item.value == "7"
            )));
        }
        _ => panic!("expected Nested entry"),
    }
}

#[tokio::test]
async fn test_typed_nested_calldata_selector_path_and_chain_id_path_render() {
    let inner_sig = decoder::parse_signature("consume()").unwrap();
    let inner_selector = format!("0x{}", hex::encode(inner_sig.selector));
    let inner_addr = "0x1000000000000000000000000000000000000001";

    let descriptor = Descriptor::from_json(
        r#"{
            "context": { "eip712": { "deployments": [{"chainId": 1, "address": "0xabc"}] } },
            "metadata": { "owner": "test", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "Relay(address target,uint256 targetChainId,bytes4 selector,bytes data)": {
                        "intent": "Relay",
                        "fields": [{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {
                                "calleePath": "target",
                                "chainIdPath": "targetChainId",
                                "selectorPath": "selector"
                            }
                        }]
                    }
                }
            }
        }"#,
    )
    .unwrap();
    let inner = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 10, "address": "0x1000000000000000000000000000000000000001"}] } },
            "metadata": { "owner": "inner", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "consume()": {
                        "intent": "Consume",
                        "fields": []
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Relay": [
                { "name": "target", "type": "address" },
                { "name": "targetChainId", "type": "uint256" },
                { "name": "selector", "type": "bytes4" },
                { "name": "data", "type": "bytes" }
            ]
        },
        "primaryType": "Relay",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": {
            "target": inner_addr,
            "targetChainId": 10,
            "selector": inner_selector,
            "data": "0x"
        }
    }))
    .unwrap();

    let descriptors = vec![
        wrap_rd(descriptor, 1, "0xabc").into_iter().next().unwrap(),
        wrap_rd(inner, 10, inner_addr).into_iter().next().unwrap(),
    ];
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    match &result.entries[0] {
        DisplayEntry::Nested { intent, .. } => assert_eq!(intent, "Consume"),
        _ => panic!("expected Nested entry"),
    }
}

#[tokio::test]
async fn test_typed_nested_calldata_constant_params_render() {
    let inner_sig = decoder::parse_signature("consume()").unwrap();
    let inner_selector = format!("0x{}", hex::encode(inner_sig.selector));
    let inner_addr = "0x1000000000000000000000000000000000000001";
    let spender = "0x2000000000000000000000000000000000000002";

    let descriptor = Descriptor::from_json(&format!(
        r#"{{
            "context": {{ "eip712": {{ "deployments": [{{"chainId": 1, "address": "0xabc"}}] }} }},
            "metadata": {{ "owner": "test", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
            "display": {{
                "definitions": {{}},
                "formats": {{
                    "Relay(bytes data)": {{
                        "intent": "Relay",
                        "fields": [{{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {{
                                "callee": "{inner_addr}",
                                "chainId": 10,
                                "selector": "{inner_selector}",
                                "amount": "9",
                                "spender": "{spender}"
                            }}
                        }}]
                    }}
                }}
            }}
        }}"#
    ))
    .unwrap();
    let inner = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 10, "address": "0x1000000000000000000000000000000000000001"}] } },
            "metadata": { "owner": "inner", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": {
                    "consume()": {
                        "intent": "Consume",
                        "fields": [
                            { "path": "@.from", "label": "Spender", "format": "address" },
                            { "path": "@.value", "label": "Amount", "format": "number" }
                        ]
                    }
                }
            }
        }"#,
    )
    .unwrap();

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [],
            "Relay": [{ "name": "data", "type": "bytes" }]
        },
        "primaryType": "Relay",
        "domain": { "chainId": 1, "verifyingContract": "0xabc" },
        "message": { "data": "0x" }
    }))
    .unwrap();

    let descriptors = vec![
        wrap_rd(descriptor, 1, "0xabc").into_iter().next().unwrap(),
        wrap_rd(inner, 10, inner_addr).into_iter().next().unwrap(),
    ];
    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    match &result.entries[0] {
        DisplayEntry::Nested { entries, .. } => {
            assert!(entries.iter().any(|entry| matches!(
                entry,
                DisplayEntry::Item(item) if item.label == "Spender" && item.value == spender
            )));
            assert!(entries.iter().any(|entry| matches!(
                entry,
                DisplayEntry::Item(item) if item.label == "Amount" && item.value == "9"
            )));
        }
        _ => panic!("expected Nested entry"),
    }
}

#[tokio::test]
async fn test_nested_calldata_conflicting_constant_and_path_params_error() {
    let selector = format!(
        "0x{}",
        hex::encode(decoder::parse_signature("consume()").unwrap().selector)
    );
    let descriptor = Descriptor::from_json(&format!(
        r#"{{
            "context": {{ "contract": {{ "deployments": [{{"chainId": 1, "address": "0xabc"}}] }} }},
            "metadata": {{ "owner": "test", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
            "display": {{
                "definitions": {{}},
                "formats": {{
                    "outer(bytes data)": {{
                        "intent": "Outer",
                        "fields": [{{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {{
                                "callee": "0x1000000000000000000000000000000000000001",
                                "calleePath": "missing",
                                "selector": "{selector}"
                            }}
                        }}]
                    }}
                }}
            }}
        }}"#
    ))
    .unwrap();
    let calldata = build_single_bytes_calldata("outer(bytes)", &[]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let err = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("both constant and path forms"));
}

#[tokio::test]
async fn test_nested_calldata_malformed_constant_params_error() {
    let calldata = build_single_bytes_calldata("outer(bytes)", &[]);
    let cases = [
        (r#""callee": "0x1234", "selector": "0x12345678""#, "callee"),
        (
            r#""callee": "0x1000000000000000000000000000000000000001", "selector": "0x1234""#,
            "selector",
        ),
        (
            r#""callee": "0x1000000000000000000000000000000000000001", "selector": "0x12345678", "amount": "not-a-number""#,
            "amount",
        ),
    ];

    for (params_body, expected_param) in cases {
        let descriptor = Descriptor::from_json(&format!(
            r#"{{
                "context": {{ "contract": {{ "deployments": [{{"chainId": 1, "address": "0xabc"}}] }} }},
                "metadata": {{ "owner": "test", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
                "display": {{
                    "definitions": {{}},
                    "formats": {{
                        "outer(bytes data)": {{
                            "intent": "Outer",
                            "fields": [{{
                                "path": "data",
                                "label": "Inner",
                                "format": "calldata",
                                "params": {{ {params_body} }}
                            }}]
                        }}
                    }}
                }}
            }}"#
        ))
        .unwrap();

        let tx = TransactionContext {
            chain_id: 1,
            to: "0xabc",
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };
        let err = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected_param), "{err}");
    }
}

#[tokio::test]
async fn test_unresolved_nested_callee_falls_back_to_raw_preview() {
    let selector = format!(
        "0x{}",
        hex::encode(decoder::parse_signature("consume()").unwrap().selector)
    );
    let descriptor = Descriptor::from_json(&format!(
        r#"{{
            "context": {{ "contract": {{ "deployments": [{{"chainId": 1, "address": "0xabc"}}] }} }},
            "metadata": {{ "owner": "test", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
            "display": {{
                "definitions": {{}},
                "formats": {{
                    "outer(bytes data)": {{
                        "intent": "Outer",
                        "fields": [{{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {{
                                "calleePath": "missing",
                                "selector": "{selector}"
                            }}
                        }}]
                    }}
                }}
            }}
        }}"#
    ))
    .unwrap();

    let calldata = build_single_bytes_calldata("outer(bytes)", &[]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&wrap_rd(descriptor, 1, "0xabc"), &tx, &EmptyDataProvider)
        .await
        .unwrap();

    match &result.entries[0] {
        DisplayEntry::Nested { intent, .. } => {
            assert!(intent.starts_with("Unknown function"));
        }
        _ => panic!("expected Nested entry"),
    }
}

#[tokio::test]
async fn test_resolver_finds_nested_descriptor_with_constant_callee_and_chain_id() {
    let inner_sig = decoder::parse_signature("consume()").unwrap();
    let selector = format!("0x{}", hex::encode(inner_sig.selector));
    let outer = Descriptor::from_json(&format!(
        r#"{{
            "context": {{ "contract": {{ "deployments": [{{"chainId": 1, "address": "0xabc"}}] }} }},
            "metadata": {{ "owner": "test", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
            "display": {{
                "definitions": {{}},
                "formats": {{
                    "outer(bytes data)": {{
                        "intent": "Outer",
                        "fields": [{{
                            "path": "data",
                            "label": "Inner",
                            "format": "calldata",
                            "params": {{
                                "callee": "0x1000000000000000000000000000000000000001",
                                "chainId": 10,
                                "selector": "{selector}"
                            }}
                        }}]
                    }}
                }}
            }}
        }}"#
    ))
    .unwrap();
    let inner = Descriptor::from_json(
        r#"{
            "context": { "contract": { "deployments": [{"chainId": 10, "address": "0x1000000000000000000000000000000000000001"}] } },
            "metadata": { "owner": "inner", "enums": {}, "constants": {}, "maps": {} },
            "display": {
                "definitions": {},
                "formats": { "consume()": { "intent": "Consume", "fields": [] } }
            }
        }"#,
    )
    .unwrap();

    let mut source = erc7730::resolver::StaticSource::new();
    source.add_calldata(1, "0xabc", outer);
    source.add_calldata(10, "0x1000000000000000000000000000000000000001", inner);

    let calldata = build_single_bytes_calldata("outer(bytes)", &[]);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0xabc",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let descriptors = erc7730::resolve_descriptors_for_tx(&tx, &source)
        .await
        .unwrap();
    assert_eq!(descriptors.len(), 2);
    assert_eq!(
        descriptors[1].address,
        "0x1000000000000000000000000000000000000001"
    );
    assert_eq!(descriptors[1].chain_id, 10);
}
