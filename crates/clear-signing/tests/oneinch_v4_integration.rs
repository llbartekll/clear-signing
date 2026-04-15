//! Integration tests using real 1inch AggregationRouterV4 transactions (Ethereum).
#![allow(non_snake_case)]

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, DisplayEntry, TransactionContext};

fn load_descriptor() -> Descriptor {
    let path = format!(
        "{}/tests/fixtures/calldata-AggregationRouterV4-eth.json",
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
        "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        TokenMeta { symbol: "ETH".to_string(), decimals: 18, name: "Ether".to_string() },
    );
    custom.insert(
        1,
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
        TokenMeta { symbol: "WETH".to_string(), decimals: 18, name: "Wrapped Ether".to_string() },
    );
    custom.insert(
        1,
        "0x3845badade8e6dff049820680d1f14bd3903a5d0",
        TokenMeta { symbol: "SAND".to_string(), decimals: 18, name: "The Sandbox".to_string() },
    );
    custom.insert(
        1,
        "0x0f2d719407fdbeff09d87557abb7232601fd9f29",
        TokenMeta { symbol: "SYN".to_string(), decimals: 18, name: "Synapse".to_string() },
    );
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

const CONTRACT: &str = "0x1111111254fb6c44bac0bed2854e76f90643097d";

fn decode_hex(s: &str) -> Vec<u8> {
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
async fn smoke_1inch_v4_swap_eth_to_sand() {
    // Real tx: 0x6ce518ab81fa4852c4bb12bb9c1871c37897a5e75558b942b88e73078ca96fbd
    // ETH → SAND, value=2500000000000000 (0.0025 ETH)
    let descriptor = load_descriptor();
    let hex_path = format!(
        "{}/tests/fixtures/1inch-v4-swap-eth-sand.hex",
        env!("CARGO_MANIFEST_DIR")
    );
    let hex_str = std::fs::read_to_string(&hex_path).expect("read hex fixture");
    let calldata = hex::decode(hex_str.trim()).expect("valid hex");
    let val = value_bytes("2500000000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0x89c9e8bd41b4fa23a83f9e4169744ca4c8e2e4ec"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swap ETH→SAND ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => eprintln!("  [field] {}: {}", item.label, item.value),
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items { eprintln!("    [{}]: {}", item.label, item.value); }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() { eprintln!("  diagnostics: {:?}", model.diagnostics()); }
        }
        Err(e) => eprintln!("FAIL swap ETH→SAND: {e}"),
    }
    assert!(result.is_ok(), "swap ETH→SAND failed: {:?}", result.err());
}

#[tokio::test]
async fn smoke_1inch_v4_swap_syn_to_eth() {
    // Real tx: 0xfa42b88629b911345c72c0b29bdf697731632a50280e5cb0c3c2cd222dd8eedd
    // SYN → ETH, value=0
    let descriptor = load_descriptor();
    let calldata = decode_hex("7c0252000000000000000000000000005141b82f5ffda4c6fe1e372978f1c5427640a190000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000001800000000000000000000000000f2d719407fdbeff09d87557abb7232601fd9f29000000000000000000000000eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee0000000000000000000000005141b82f5ffda4c6fe1e372978f1c5427640a190000000000000000000000000634d1cc5bd75b7d4bcdeae4b9a1cb4cf81f3dcba000000000000000000000000000000000000000000000004b4fee4c953eafd09000000000000000000000000000000000000000000000000002a0d1d047d5b3400000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001a300000000000000000000000000000000016500014f0001050000c900004e00a0744c8c090f2d719407fdbeff09d87557abb7232601fd9f29c566940cf6ddbf6836185a6ba9edf2c00e84d362000000000000000000000000000000000000000000000000181993ef8beb198a0c200f2d719407fdbeff09d87557abb7232601fd9f294a86c01d67965f8cb3d0aaa2c655705e64097c316ae4071198002dc6c04a86c01d67965f8cb3d0aaa2c655705e64097c31000000000000000000000000000000000000000000000000002a0d1d047d5b340f2d719407fdbeff09d87557abb7232601fd9f294101c02aaa39b223fe8d0a0e5c4f27ead9083c756cc200042e1a7d4d000000000000000000000000000000000000000000000000000000000000000000a0f2fa6b66eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee000000000000000000000000000000000000000000000000002a79da140cd83f00000000000000000006b8b5b2b2edc9c0611111111254fb6c44bac0bed2854e76f90643097d000000000000000000000000000000000000000000000004b4fee4c953eafd0900000000000000000000000000000000000000000000000000000000003db5cd3b");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x634d1cc5bd75b7d4bcdeae4b9a1cb4cf81f3dcba"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swap SYN→ETH ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => eprintln!("  [field] {}: {}", item.label, item.value),
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items { eprintln!("    [{}]: {}", item.label, item.value); }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() { eprintln!("  diagnostics: {:?}", model.diagnostics()); }
        }
        Err(e) => eprintln!("FAIL swap SYN→ETH: {e}"),
    }
    assert!(result.is_ok(), "swap SYN→ETH failed: {:?}", result.err());
}

// ── unoswap ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_1inch_v4_unoswap_weth() {
    // Real tx: 0x65ea2da89f1fdc2beb32afc76ea100fec1cb2759edfa375ba474c62086c5dbd0
    // WETH → ?, value=0
    let descriptor = load_descriptor();
    let calldata = decode_hex("2e95b6c8000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000000000000000000000000000008e1bc9bf040000000000000000000000000000000000000000000000000000000ff973cafa80000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000180000000000000003b6d0340067992fbc56b08e7258f938c1d237bec3783307c");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x44c1a64d827b129fd4efc3fd49b181093f52ffb5"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== unoswap WETH ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => eprintln!("  [field] {}: {}", item.label, item.value),
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items { eprintln!("    [{}]: {}", item.label, item.value); }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() { eprintln!("  diagnostics: {:?}", model.diagnostics()); }
        }
        Err(e) => eprintln!("FAIL unoswap WETH: {e}"),
    }
    assert!(result.is_ok(), "unoswap WETH failed: {:?}", result.err());
}

#[tokio::test]
async fn smoke_1inch_v4_unoswap_weth_1() {
    // Real tx: 0xab0281cb9dfaf2971d9f9e1bec1f26f0365455c2f722043ab693a4a147398be8
    // WETH → ?, value=0
    let descriptor = load_descriptor();
    let calldata = decode_hex("2e95b6c8000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000000000000000000000000000008e1bc9bf040000000000000000000000000000000000000000000000000000001ff973cafa80000000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000000000000000000180000000000000003b6d0340067992fbc56b08e7258f938c1d237bec3783307c");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x44c1a64d827b129fd4efc3fd49b181093f52ffb5"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== unoswap WETH #1 ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
            eprintln!("  owner: {:?}", model.owner);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => eprintln!("  [field] {}: {}", item.label, item.value),
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  [group] {label}:");
                        for item in items { eprintln!("    [{}]: {}", item.label, item.value); }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.diagnostics().is_empty() { eprintln!("  diagnostics: {:?}", model.diagnostics()); }
        }
        Err(e) => eprintln!("FAIL unoswap WETH #1: {e}"),
    }
    assert!(result.is_ok(), "unoswap WETH #1 failed: {:?}", result.err());
}
