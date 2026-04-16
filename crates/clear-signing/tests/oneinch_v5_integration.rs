//! Integration tests using real 1inch AggregationRouterV5 transactions (Ethereum).
#![allow(non_snake_case)]

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{
    CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource,
};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, DisplayEntry, FormatOutcome, TransactionContext};

fn load_descriptor() -> Descriptor {
    let path = format!(
        "{}/tests/fixtures/calldata-AggregationRouterV5.json",
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
        TokenMeta {
            symbol: "ETH".to_string(),
            decimals: 18,
            name: "Ether".to_string(),
        },
    );
    custom.insert(
        1,
        "0xdac17f958d2ee523a2206206994597c13d831ec7",
        TokenMeta {
            symbol: "USDT".to_string(),
            decimals: 6,
            name: "Tether USD".to_string(),
        },
    );
    custom.insert(
        1,
        "0x8e42fe26fc1697f57076c9f2a8d1ff69cf7f6fda",
        TokenMeta {
            symbol: "AGURI".to_string(),
            decimals: 9,
            name: "Aguri-Chan".to_string(),
        },
    );
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

const CONTRACT: &str = "0x1111111254eeb25477b68fb85ed929f73a960582";

fn load_hex_fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let hex_str = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    hex::decode(hex_str.trim()).expect("valid hex")
}

fn decode_hex(s: &str) -> Vec<u8> {
    hex::decode(s).expect("valid hex")
}

fn value_bytes(wei: &str) -> Vec<u8> {
    let n = wei.parse::<u128>().expect("valid decimal");
    let be = n.to_be_bytes();
    let start = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    be[start..].to_vec()
}

fn print_model(model: &FormatOutcome, label: &str) {
    eprintln!("=== {label} ===");
    eprintln!("  intent: {:?}", model.intent);
    eprintln!("  interpolated_intent: {:?}", model.interpolated_intent);
    eprintln!("  owner: {:?}", model.owner);
    for entry in &model.entries {
        match entry {
            DisplayEntry::Item(item) => eprintln!("  [field] {}: {}", item.label, item.value),
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

// ── swap ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_1inch_v5_swap_eth_to_aguri() {
    // Real tx: 0xb8d9d6e994e971ddca1ea55032dc0f236913a85ef11d9c80ec3c6413251e07bb
    // ETH → AGURI, value=6700000000000000
    let descriptor = load_descriptor();
    let calldata = load_hex_fixture("1inch-v5-swap_0.hex");
    let val = value_bytes("6700000000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0x65c121086ee77e298c9c74b633d5585c5e2ff26a"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    if let Ok(model) = &result {
        print_model(model, "swap ETH→AGURI");
    } else {
        eprintln!("FAIL: {:?}", result.as_ref().err());
    }
    assert!(result.is_ok(), "swap ETH→AGURI failed: {:?}", result.err());
}

#[tokio::test]
async fn smoke_1inch_v5_swap_eth_to_usdt() {
    // Real tx: 0xcaeeada2c8a019c6c485f5a09f0992dc134c4e9fb76fd9be93ee4959f3a64899
    // ETH → USDT, value=44041370000000000
    let descriptor = load_descriptor();
    let calldata = load_hex_fixture("1inch-v5-swap_1.hex");
    let val = value_bytes("44041370000000000");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: Some(&val),
        from: Some("0xdb27c96e02b9aa6cc8a92c8bc12c78f3d910fd32"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    if let Ok(model) = &result {
        print_model(model, "swap ETH→USDT");
    } else {
        eprintln!("FAIL: {:?}", result.as_ref().err());
    }
    assert!(result.is_ok(), "swap ETH→USDT failed: {:?}", result.err());
}

// ── uniswapV3Swap ────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_1inch_v5_uniswapV3Swap_0() {
    // Real tx: 0x18eb65c89f1bc43758c0f05b8f29ff81ba7d5f8cbfd8ba5be13a681bdce3fece
    // No explicit srcToken/dstToken in this function — amount/minReturn only
    let descriptor = load_descriptor();
    let calldata = decode_hex("e449022e00000000000000000000000000000000000000000000000001d704cebc5240000000000000000000000000000000000000000000000000f2ff65c331b300000000000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000001000000000000000000000000ec2061372a02d5e416f5d8905eea64cab2c10970");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x65a8f07bd9a8598e1b5b6c0a88f4779dbc077675"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    if let Ok(model) = &result {
        print_model(model, "uniswapV3Swap #0");
    } else {
        eprintln!("FAIL: {:?}", result.as_ref().err());
    }
    assert!(
        result.is_ok(),
        "uniswapV3Swap #0 failed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn smoke_1inch_v5_uniswapV3Swap_1() {
    // Real tx: 0x59ffc54fb11cc8eeb5a9a9d19c03425ff5d84df02346b6226ecdd5eda9887c3d
    let descriptor = load_descriptor();
    let calldata = decode_hex("e449022e00000000000000000000000000000000000000000000000018cf2fada8f740000000000000000000000000000000000000000000000000383eb1348c3af00000000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000018000000000000000000000001d42064fc4beb5f8aaf85f4617ae8b3b5b8bd801");
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, CONTRACT);
    let tx = TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: Some("0x65a8f07bd9a8598e1b5b6c0a88f4779dbc077675"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await;
    if let Ok(model) = &result {
        print_model(model, "uniswapV3Swap #1");
    } else {
        eprintln!("FAIL: {:?}", result.as_ref().err());
    }
    assert!(
        result.is_ok(),
        "uniswapV3Swap #1 failed: {:?}",
        result.err()
    );
}
