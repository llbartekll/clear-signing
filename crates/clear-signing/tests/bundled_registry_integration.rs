//! End-to-end integration tests for the embedded registry snapshot
//! (`bundled-registry` feature): resolve descriptors fully offline via
//! `BundledRegistrySource`, then format real calldata with them.

#![cfg(feature = "bundled-registry")]

use clear_signing::token::{CompositeDataProvider, WellKnownTokenSource};
use clear_signing::{
    format_calldata, resolve_descriptors_for_tx, BundledRegistrySource, DisplayEntry,
    ResolvedDescriptorResolution, TransactionContext,
};

const USDT_MAINNET: &str = "0xdac17f958d2ee523a2206206994597c13d831ec7";

fn decode_hex(hex_str: &str) -> Vec<u8> {
    let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(s).unwrap_or_else(|e| panic!("invalid hex '{hex_str}': {e}"))
}

/// transfer(0x000…001, 1000000) — 1 USDT.
fn usdt_transfer_calldata() -> Vec<u8> {
    decode_hex(
        "a9059cbb000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000f4240",
    )
}

#[tokio::test]
async fn bundled_resolve_and_format_usdt_transfer() {
    let source = BundledRegistrySource::new().expect("embedded snapshot must parse");
    let calldata = usdt_transfer_calldata();
    let tx = TransactionContext {
        chain_id: 1,
        to: USDT_MAINNET,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let resolution = resolve_descriptors_for_tx(&tx, &source, None)
        .await
        .expect("resolution must not error");
    let descriptors = match resolution {
        ResolvedDescriptorResolution::Found(descriptors) => descriptors,
        ResolvedDescriptorResolution::NotFound => {
            panic!("USDT descriptor must be present in the bundled snapshot")
        }
    };
    assert!(!descriptors.is_empty());

    let provider = CompositeDataProvider::new(vec![Box::new(WellKnownTokenSource::new())]);
    let model = format_calldata(&descriptors, &tx, &provider)
        .await
        .expect("formatting must succeed with bundled descriptor");

    assert_eq!(model.intent, "Send");
    let amount = model
        .entries
        .iter()
        .find_map(|entry| match entry {
            DisplayEntry::Item(item) if item.label == "Amount" => Some(item.value.clone()),
            _ => None,
        })
        .expect("Amount entry must be rendered");
    assert!(
        amount.contains("USDT"),
        "amount should be token-formatted, got: {amount}"
    );
    assert!(
        amount.contains('1'),
        "1 USDT expected in amount, got: {amount}"
    );
}

#[tokio::test]
async fn bundled_resolution_not_found_for_unknown_contract() {
    let source = BundledRegistrySource::new().expect("embedded snapshot must parse");
    let calldata = usdt_transfer_calldata();
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x000000000000000000000000000000000000dead",
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let resolution = resolve_descriptors_for_tx(&tx, &source, None)
        .await
        .expect("resolution must not error");
    assert!(matches!(resolution, ResolvedDescriptorResolution::NotFound));
}
