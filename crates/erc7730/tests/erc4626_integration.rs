//! Integration tests for ERC-4626 vault operations using real on-chain transactions.
//! Covers LeadBlock USDC RWA vault and Yield.xyz USDe vault.

use erc7730::resolver::ResolvedDescriptor;
use erc7730::token::{CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource};
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_calldata, DisplayEntry, DisplayModel, TransactionContext};

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

fn get_entry_value(model: &DisplayModel, label: &str) -> String {
    for entry in &model.entries {
        if let DisplayEntry::Item(item) = entry {
            if item.label == label {
                return item.value.clone();
            }
        }
    }
    panic!("no entry with label '{label}' found");
}

fn erc4626_token_source() -> CompositeDataProvider {
    let mut custom = StaticTokenSource::new();
    // LeadBlock USDC RWA
    custom.insert(
        1,
        "0x4ca0e178c94f039d7f202e09d8d1a655ed3fb6b6",
        TokenMeta {
            symbol: "USDC RWA".to_string(),
            decimals: 18,
            name: "LeadBlock USDC RWA".to_string(),
        },
    );
    // USDC (underlying for LeadBlock)
    custom.insert(
        1,
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );
    // Yield.xyz USDe
    custom.insert(
        1,
        "0x4c9edd5852cd905f086c759e8383e09bff1e68b3",
        TokenMeta {
            symbol: "USDe".to_string(),
            decimals: 18,
            name: "Ethena USDe".to_string(),
        },
    );
    // Yield.xyz stk-USDe
    custom.insert(
        1,
        "0x2d152fb171353e70e45322d32bc748f8a61d9971",
        TokenMeta {
            symbol: "stk-USDe".to_string(),
            decimals: 18,
            name: "StakeKit Ethena USDe Vault".to_string(),
        },
    );
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

// --- LeadBlock USDC RWA vault tests ---

const LEADBLOCK_ADDR: &str = "0x4ca0e178c94f039d7f202e09d8d1a655ed3fb6b6";

#[tokio::test]
async fn leadblock_deposit() {
    let descriptor = load_descriptor("calldata-leadblock-USDC-RWA.json");
    let descriptors = wrap_rd(descriptor, 1, LEADBLOCK_ADDR);
    let calldata = decode_hex("0x6e553f650000000000000000000000000000000000000000000000000000000005f5e1000000000000000000000000004a9ffdbc1c8d6aabafce2b6afed90e714487ea74");
    let provider = erc4626_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: LEADBLOCK_ADDR,
        calldata: &calldata,
        value: None,
        from: Some("0x77e75c2a92061c9c61282d04495766ca6f98784e"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

    assert_eq!(result.intent, "Deposit");
    assert_eq!(result.owner.as_deref(), Some("LeadBlock"));
    assert_eq!(get_entry_value(&result, "Deposit asset"), "100 USDC");
    assert_eq!(get_entry_value(&result, "Share ticker"), "USDC RWA");
    assert_eq!(
        get_entry_value(&result, "Send shares to"),
        "0x4a9fFDBc1c8d6aAbAfce2b6AFED90E714487ea74"
    );
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn leadblock_mint() {
    let descriptor = load_descriptor("calldata-leadblock-USDC-RWA.json");
    let descriptors = wrap_rd(descriptor, 1, LEADBLOCK_ADDR);
    let calldata = decode_hex("0x94bf804d000000000000000000000000000000000000000000000000000000003b9aca00000000000000000000000000000000000000000000000000000000000000dead");
    let provider = erc4626_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: LEADBLOCK_ADDR,
        calldata: &calldata,
        value: None,
        from: Some("0x59efacc8668e6918e1d9930768402a41e794404b"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

    assert_eq!(result.intent, "Mint");
    assert_eq!(get_entry_value(&result, "Deposit asset"), "USDC");
    assert_eq!(
        get_entry_value(&result, "Minted shares"),
        "0.000000001 USDC RWA"
    );
    assert_eq!(
        get_entry_value(&result, "Mint shares to"),
        "0x000000000000000000000000000000000000dEaD"
    );
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn leadblock_withdraw() {
    let descriptor = load_descriptor("calldata-leadblock-USDC-RWA.json");
    let descriptors = wrap_rd(descriptor, 1, LEADBLOCK_ADDR);
    let calldata = decode_hex("0xb460af940000000000000000000000000000000000000000000000000000000002fb3f7b0000000000000000000000008379bd16381620914d8fa3d535f6ca9ef23ece530000000000000000000000008379bd16381620914d8fa3d535f6ca9ef23ece53");
    let provider = erc4626_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: LEADBLOCK_ADDR,
        calldata: &calldata,
        value: None,
        from: Some("0x8379bd16381620914d8fa3d535f6ca9ef23ece53"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

    assert_eq!(result.intent, "Withdraw");
    assert_eq!(
        get_entry_value(&result, "Withdraw exactly"),
        "50.020219 USDC"
    );
    assert_eq!(
        get_entry_value(&result, "To"),
        "0x8379bD16381620914d8fA3d535F6Ca9eF23ece53"
    );
    assert_eq!(
        get_entry_value(&result, "Owner"),
        "0x8379bD16381620914d8fA3d535F6Ca9eF23ece53"
    );
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn leadblock_redeem() {
    let descriptor = load_descriptor("calldata-leadblock-USDC-RWA.json");
    let descriptors = wrap_rd(descriptor, 1, LEADBLOCK_ADDR);
    let calldata = decode_hex("0xba0876520000000000000000000000000000000000000000000000008870570820358234000000000000000000000000e0a68a5650c72f2d65b8b5de386234420b90afed000000000000000000000000e0a68a5650c72f2d65b8b5de386234420b90afed");
    let provider = erc4626_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: LEADBLOCK_ADDR,
        calldata: &calldata,
        value: None,
        from: Some("0xe0a68a5650c72f2d65b8b5de386234420b90afed"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

    assert_eq!(result.intent, "Redeem");
    assert_eq!(
        get_entry_value(&result, "Shares to redeem"),
        "9.8314536789615253 USDC RWA"
    );
    assert_eq!(
        get_entry_value(&result, "To"),
        "0xe0A68A5650C72f2d65B8B5de386234420B90Afed"
    );
    assert!(result.warnings.is_empty());
}

// --- Yield.xyz USDe vault tests ---

const YIELDXYZ_ADDR: &str = "0x2d152fb171353e70e45322d32bc748f8a61d9971";

#[tokio::test]
async fn yieldxyz_deposit() {
    let descriptor = load_descriptor("calldata-yieldxyz-usde-vault.json");
    let descriptors = wrap_rd(descriptor, 1, YIELDXYZ_ADDR);
    let calldata = decode_hex("0x6e553f650000000000000000000000000000000000000000000000056bc75e2d6310000000000000000000000000000088fc6efa5e8b5637c5f263c9921b3869a4c92584");
    let provider = erc4626_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: YIELDXYZ_ADDR,
        calldata: &calldata,
        value: None,
        from: Some("0x88fc6efa5e8b5637c5f263c9921b3869a4c92584"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

    assert_eq!(result.intent, "Deposit");
    assert_eq!(result.owner.as_deref(), Some("Yield.xyz"));
    assert_eq!(get_entry_value(&result, "Deposit asset"), "100 USDe");
    assert_eq!(get_entry_value(&result, "Share ticker"), "stk-USDe");
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn yieldxyz_redeem() {
    let descriptor = load_descriptor("calldata-yieldxyz-usde-vault.json");
    let descriptors = wrap_rd(descriptor, 1, YIELDXYZ_ADDR);
    let calldata = decode_hex("0xba0876520000000000000000000000000000000000000000199c0bb1b53a26cfdbed6e1100000000000000000000000036272f3569daf11bb4464354cc136acd9eca1d2b00000000000000000000000036272f3569daf11bb4464354cc136acd9eca1d2b");
    let provider = erc4626_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: YIELDXYZ_ADDR,
        calldata: &calldata,
        value: None,
        from: Some("0x36272f3569daf11bb4464354cc136acd9eca1d2b"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();

    assert_eq!(result.intent, "Redeem");
    assert_eq!(result.owner.as_deref(), Some("Yield.xyz"));
    assert_eq!(
        get_entry_value(&result, "Shares to redeem"),
        "7925772897.557314225765707281 stk-USDe"
    );
    assert_eq!(
        get_entry_value(&result, "To"),
        "0x36272F3569DaF11Bb4464354cC136Acd9ecA1D2B"
    );
    assert!(result.warnings.is_empty());
}
