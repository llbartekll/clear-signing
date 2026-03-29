//! Integration regression tests for real Permit2 EIP-712 requests.

use erc7730::eip712::TypedData;
use erc7730::token::{StaticTokenSource, TokenMeta};
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_typed_data, DisplayEntry, ResolvedDescriptor};

fn load_descriptor(fixture: &str) -> Descriptor {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    Descriptor::from_json(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn entry_value<'a>(entries: &'a [DisplayEntry], label: &str) -> &'a str {
    entries
        .iter()
        .find_map(|entry| match entry {
            DisplayEntry::Item(item) if item.label == label => Some(item.value.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing entry with label '{label}'"))
}

#[tokio::test]
async fn permit2_permit_single_real_wallet_request_formats_with_current_descriptor() {
    let descriptor = load_descriptor("uniswap-permit2-merged.json");
    let typed_data: TypedData = serde_json::from_str(
        r#"{
            "types": {
                "PermitSingle": [
                    { "name": "details", "type": "PermitDetails" },
                    { "name": "spender", "type": "address" },
                    { "name": "sigDeadline", "type": "uint256" }
                ],
                "PermitDetails": [
                    { "name": "token", "type": "address" },
                    { "name": "amount", "type": "uint160" },
                    { "name": "expiration", "type": "uint48" },
                    { "name": "nonce", "type": "uint48" }
                ],
                "EIP712Domain": [
                    { "name": "name", "type": "string" },
                    { "name": "chainId", "type": "uint256" },
                    { "name": "verifyingContract", "type": "address" }
                ]
            },
            "domain": {
                "name": "Permit2",
                "chainId": "10",
                "verifyingContract": "0x000000000022d473030f116ddee9f6b43ac78ba3"
            },
            "primaryType": "PermitSingle",
            "message": {
                "details": {
                    "token": "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58",
                    "amount": "1461501637330902918203684832716283019655932542975",
                    "expiration": "1777370231",
                    "nonce": "0"
                },
                "spender": "0x851116d9223fabed8e56c0e6b8ad0c31d98b3507",
                "sigDeadline": "1774780031"
            }
        }"#,
    )
    .unwrap();

    let descriptors = vec![ResolvedDescriptor {
        descriptor,
        chain_id: 10,
        address: "0x000000000022d473030f116ddee9f6b43ac78ba3".to_string(),
    }];

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        10,
        "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58",
        TokenMeta {
            symbol: "USDT".to_string(),
            decimals: 6,
            name: "Tether USD".to_string(),
        },
    );

    let result = format_typed_data(&descriptors, &typed_data, &tokens)
        .await
        .unwrap();

    assert_eq!(result.intent, "Authorize spending of token");
    assert_eq!(result.owner.as_deref(), Some("Uniswap Labs"));
    assert!(result.warnings.is_empty(), "unexpected warnings: {:?}", result.warnings);

    assert_eq!(
        entry_value(&result.entries, "Spender"),
        "0x851116d9223fabed8e56c0e6b8ad0c31d98b3507"
    );
    assert_eq!(
        entry_value(&result.entries, "Amount allowance"),
        "1461501637330902918203684832716283019655932.542975 USDT"
    );
    assert_eq!(
        entry_value(&result.entries, "Approval expires"),
        "2026-04-28 09:57:11 UTC"
    );
}
