//! Integration tests using real 1inch AggregationRouterV3 transactions.
#![allow(non_snake_case)]

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, DisplayEntry, TransactionContext};

fn load_descriptor() -> Descriptor {
    let path = format!(
        "{}/tests/fixtures/calldata-AggregationRouterV3.json",
        env!("CARGO_MANIFEST_DIR")
    );
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

fn token_source() -> CompositeDataProvider {
    let mut custom = StaticTokenSource::new();
    custom.insert(
        1,
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );
    custom.insert(
        1,
        "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        TokenMeta {
            symbol: "ETH".to_string(),
            decimals: 18,
            name: "Ether".to_string(),
        },
    );
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

const CONTRACT: &str = "0x11111112542d85b3ef69ae05771c2dccff4faa26";

fn decode_hex(s: &str) -> Vec<u8> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).expect("valid hex")
}

fn value_bytes(wei: &str) -> Vec<u8> {
    let n = wei.parse::<u128>().expect("valid decimal");
    let be = n.to_be_bytes();
    let start = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    be[start..].to_vec()
}

// ── swap ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_1inch_v3_swap_eth_to_usdc() {
    // Real tx: 0x8bd3433d929a7eb4a54c8ffc6ed05190d8917e8ffe179a6bc32b222c1ffa052c
    // Swaps ETH → USDC, value=3500000000000000 (0.0035 ETH)
    let descriptor = load_descriptor();
    let hex_path = format!(
        "{}/tests/fixtures/1inch-swap-calldata.hex",
        env!("CARGO_MANIFEST_DIR")
    );
    let hex_str = std::fs::read_to_string(&hex_path).expect("read hex fixture");
    let calldata = hex::decode(hex_str.trim()).expect("valid hex");
    let val = value_bytes("3500000000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0xe9785422a26502e7336d800bd633339360851510"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swap ETH→USDC ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [field] {}: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() {
                eprintln!("  diagnostics: {:?}", model.diagnostics());
            }
        }
        Err(e) => eprintln!("FAIL swap: {e}"),
    }
    assert!(result.is_ok(), "swap failed: {:?}", result.err());
}

// ── unoswap ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_1inch_v3_unoswap_eth_swap_0() {
    // Real tx: 0x66ac80852fa53c04af9361186494c812944e07e2418585813801f5342aa3b439
    // srcToken=0x000..000 (native ETH), value=4142250000000000
    let descriptor = load_descriptor();
    let calldata = decode_hex("2e95b6c80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000eb75abbd224000000000000000000000000000000000000000000000419aff021213ce297b92f0000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000180000000000000003b6d0340da3a20aad0c34fa742bd9813d45bbf67c787ae0b0bd34b36");
    let val = value_bytes("4142250000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0xf3c0be3fcefc4e1358da237d75aad6c150b81ac4"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== unoswap #0 ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [field] {}: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() {
                eprintln!("  diagnostics: {:?}", model.diagnostics());
            }
        }
        Err(e) => eprintln!("FAIL unoswap #0: {e}"),
    }
    assert!(result.is_ok(), "unoswap #0 failed: {:?}", result.err());
}

#[tokio::test]
async fn smoke_1inch_v3_unoswap_eth_swap_1() {
    // Real tx: 0xde5ade8730c63b50bb2fa90bbee027c04655d84459cb827e689bbf72ecc1d4cc
    // srcToken=0x000..000 (native ETH), value=1850000000000000
    let descriptor = load_descriptor();
    let calldata = decode_hex("2e95b6c8000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000069290b0d5a000000000000000000000000000000000000000000000022d00617af95d21c91f560000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000180000000000000003b6d0340da3a20aad0c34fa742bd9813d45bbf67c787ae0b0bd34b36");
    let val = value_bytes("1850000000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0x3f1ef1a0f69f98addacef5cd636c2d9589d9a9ec"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== unoswap #1 ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [field] {}: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() {
                eprintln!("  diagnostics: {:?}", model.diagnostics());
            }
        }
        Err(e) => eprintln!("FAIL unoswap #1: {e}"),
    }
    assert!(result.is_ok(), "unoswap #1 failed: {:?}", result.err());
}

#[tokio::test]
async fn smoke_1inch_v3_unoswap_eth_swap_2() {
    // Real tx: 0xcd5c57dff05063507e04349f59d5b394b30c6c1234a93b1d3f6b34f72404744f
    // srcToken=0x000..000 (native ETH), value=4000000000000000
    let descriptor = load_descriptor();
    let calldata = decode_hex("2e95b6c80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e35fa931a00000000000000000000000000000000000000000000000089d8147df6f8399944a70000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000180000000000000003b6d034024d3dd4a62e29770cf98810b09f89d3a90279e7a0bd34b36");
    let val = value_bytes("4000000000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0x3570ba5913767a2516d9e7209d3aa058c4d01b16"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== unoswap #2 ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [field] {}: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() {
                eprintln!("  diagnostics: {:?}", model.diagnostics());
            }
        }
        Err(e) => eprintln!("FAIL unoswap #2: {e}"),
    }
    assert!(result.is_ok(), "unoswap #2 failed: {:?}", result.err());
}
