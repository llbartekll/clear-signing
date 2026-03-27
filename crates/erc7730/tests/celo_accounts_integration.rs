//! Integration tests for Celo Accounts (Celo mainnet) using real on-chain transactions.

use erc7730::provider::EmptyDataProvider;
use erc7730::resolver::ResolvedDescriptor;
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_calldata, DisplayEntry, TransactionContext};

fn load_descriptor(fixture: &str) -> Descriptor {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    Descriptor::from_json(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn wrap_rd(descriptor: Descriptor, chain_id: u64, address: &str) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor,
        chain_id,
        address: address.to_lowercase(),
    }]
}

fn decode_hex(hex_str: &str) -> Vec<u8> {
    let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(s).unwrap_or_else(|e| panic!("invalid hex '{hex_str}': {e}"))
}

fn value_bytes(hex_str: &str) -> Option<Vec<u8>> {
    let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if s == "0" || s.is_empty() {
        return None;
    }
    let even = if s.len() % 2 != 0 {
        format!("0{s}")
    } else {
        s.to_string()
    };
    let raw = hex::decode(&even).unwrap_or_else(|e| panic!("invalid value hex '{hex_str}': {e}"));
    if raw.is_empty() || raw.iter().all(|&b| b == 0) {
        return None;
    }
    let mut padded = vec![0u8; 32usize.saturating_sub(raw.len())];
    padded.extend_from_slice(&raw);
    Some(padded)
}

fn entry_value<'a>(entries: &'a [DisplayEntry], label: &str) -> &'a str {
    for entry in entries {
        if let DisplayEntry::Item(item) = entry {
            if item.label == label {
                return &item.value;
            }
        }
    }
    panic!("missing entry '{label}'");
}

const CELO_ACCOUNTS_ADDR: &str = "0x7d21685C17607338b313a7174bAb6620baD0aaB7";

async fn run_celo_accounts_test(
    calldata_hex: &str,
    value_hex: &str,
    from: &str,
) -> erc7730::DisplayModel {
    let descriptor = load_descriptor("calldata-celo_accounts.json");
    let descriptors = wrap_rd(descriptor, 42220, CELO_ACCOUNTS_ADDR);
    let calldata = decode_hex(calldata_hex);
    let value = value_bytes(value_hex);
    let tx = TransactionContext {
        chain_id: 42220,
        to: CELO_ACCOUNTS_ADDR,
        calldata: &calldata,
        value: value.as_deref(),
        from: Some(from),
        implementation_address: None,
    };
    format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap()
}

#[tokio::test]
async fn celo_accounts_create_account_formats() {
    let result = run_celo_accounts_test(
        "0x9dca362f",
        "0x0",
        "0xebb21a1e1c7f456efb42add2fa31f0f19b4ce8bc",
    )
    .await;

    assert_eq!(result.intent, "Create Account");
    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(
        entry_value(&result.entries, "Account Owner"),
        "0xebB21A1e1c7f456Efb42add2Fa31F0f19b4CE8BC"
    );
}

#[tokio::test]
async fn celo_accounts_authorize_vote_signer_formats() {
    let result = run_celo_accounts_test(
        "0x4282ee6d0000000000000000000000004797e71f1cdb12a43a64954e67a5ef19bb2e0823000000000000000000000000000000000000000000000000000000000000001b0788c4862ad42141c0753de01017420b6c10ad14c8560fea57f0ad7b2cc175ff147d6a147172e8d2e2eab9d6322465fd74865c8b99c452fdf7180ec1e7995bb8",
        "0x0",
        "0x952aea61cb4d2062c18d48e0eb9732571cc8d2c1",
    )
    .await;

    assert_eq!(result.intent, "Authorize & Set Vote");
    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(
        entry_value(&result.entries, "Authorized Signer"),
        "0x4797E71F1CdB12A43a64954E67a5EF19bb2e0823"
    );
}

#[tokio::test]
async fn celo_accounts_set_name_formats() {
    let result = run_celo_accounts_test(
        "0xc47f00270000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000e416e676f506c75732d6e6f646531000000000000000000000000000000000000",
        "0x0",
        "0xa9a1aa35fd1fa6fd02f64432c23c7c8a3506627f",
    )
    .await;

    assert_eq!(result.intent, "Set Account Name");
    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(entry_value(&result.entries, "Name"), "AngoPlus-node1");
}
