//! Integration tests using real ParaSwap v6.2 transactions.
#![allow(non_snake_case)]

use erc7730::resolver::ResolvedDescriptor;
use erc7730::token::{CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource};
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_calldata, DisplayEntry, TransactionContext};

fn load_descriptor() -> Descriptor {
    let path = format!("{}/tests/fixtures/paraswap-v6.2.json", env!("CARGO_MANIFEST_DIR"));
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
    custom.insert(1, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", TokenMeta { symbol: "USDC".to_string(), decimals: 6, name: "USD Coin".to_string() });
    custom.insert(1, "0xdac17f958d2ee523a2206206994597c13d831ec7", TokenMeta { symbol: "USDT".to_string(), decimals: 6, name: "Tether USD".to_string() });
    custom.insert(1, "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", TokenMeta { symbol: "WETH".to_string(), decimals: 18, name: "Wrapped Ether".to_string() });
    custom.insert(1, "0x6b175474e89094c44da98b954eedeac495271d0f", TokenMeta { symbol: "DAI".to_string(), decimals: 18, name: "Dai Stablecoin".to_string() });
    custom.insert(1, "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", TokenMeta { symbol: "WBTC".to_string(), decimals: 8, name: "Wrapped BTC".to_string() });
    custom.insert(1, "0x514910771af9ca656af840dff83e8264ecf986ca", TokenMeta { symbol: "LINK".to_string(), decimals: 18, name: "ChainLink Token".to_string() });
    custom.insert(1, "0x7f39c581f595b53c5cb19bd0b3f8da6c935e2ca0", TokenMeta { symbol: "wstETH".to_string(), decimals: 18, name: "Wrapped stETH".to_string() });
    custom.insert(1, "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", TokenMeta { symbol: "ETH".to_string(), decimals: 18, name: "Ether".to_string() });
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}


#[tokio::test]
async fn test_paraswap_v6_swapExactAmountIn() {
    // Real tx: 0xfbb255412de854d9ba5f1a3b13b1cf5120c02f3ae5a8b46fe2d358e47cd99d63
    let descriptor = load_descriptor();
    let calldata = hex::decode("e3ead59e000000000000000000000000000010036c0190e009a000d0fc3541100a07380a000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec70000000000000000000000000000000000000000000000000000000016f9a2ce0000000000000000000000000000000000000000000000000000000016f9db7f0000000000000000000000000000000000000000000000000000000016fb08b679abf65f0a3e4601b1459e2612b21c3500000000000000000000000001793399000000000000000000000000000000000000000000000000000000000000000008a3c2a819e3de7aca384c798269b3ce1cd0e437900000000000000000000000000000000000000000000000000000000000000000000000000000000000016000000000000000000000000000000000000000000000000000000000000001800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000120d26f20001a72a18c002b00e6710000d68700ce00000000c000a50000ff00000300000000000000000000000000000000000000000000000000000000ce080244000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec700000000000000000000000000000000000000000000a7c5ac471b488000003200000000000000000000000000000000000000000000000016f9a2ce000000000000000000000000000000006a000f20005980200259b80c5102003040001068").expect("valid hex");
    let value: Option<&[u8]> = None;
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0x6ab0aee5f4baf26b6df64b426dd39cfc0e32e124"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapExactAmountIn ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapExactAmountIn failed: {e}"),
    }
    assert!(result.is_ok(), "swapExactAmountIn should format successfully");
}

#[tokio::test]
async fn test_paraswap_v6_swapExactAmountInOnUniswapV2() {
    // Real tx: 0x90f78f05a26a2a1dc85b103c0cfa1184a01f1c283604a0212093eac5af027dc8
    let descriptor = load_descriptor();
    let calldata = hex::decode("e8bb3b6c0000000000000000000000000000000000000000000000000000000000000060cfd59c0f530db36eea8ccbfe744f01fe3556925e18000000000000000000004300000000000000000000000000000000000000000000000000000000000001c0000000000000000000000000eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee0000000000000000000000001ad797daa9034a34e634b398c82b25f249280673000000000000000000000000000000000000000000000000006a94d74f430000000000000000000000000000000000000000000000000001e134fd032a66ae8e000000000000000000000000000000000000000000000001e3a00749ef7033f08397af8a2f6249efbd21d3a954e09d15000000000000000000000000017933330000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000401ad797daa9034a34e634b398c82b25f249280673c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").expect("valid hex");
    let value_bytes = hex::decode("6a94d74f430000").unwrap();
    let value: Option<&[u8]> = Some(&value_bytes);
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0xad05a594cdcab4bda8aeef433ac30b6ae246dd60"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapExactAmountInOnUniswapV2 ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapExactAmountInOnUniswapV2 failed: {e}"),
    }
    assert!(result.is_ok(), "swapExactAmountInOnUniswapV2 should format successfully");
}

#[tokio::test]
async fn test_paraswap_v6_swapExactAmountInOnUniswapV3() {
    // Real tx: 0x42d6b26a20716b75c0c0e5c25617d86a80426b7bacb5bd9f3e78dd3cbe05b49f
    let descriptor = load_descriptor();
    let calldata = hex::decode("876a02f6000000000000000000000000000000000000000000000000000000000000006008a3c2a819e3de7aca384c798269b3ce1cd0e4379000000000000000000000000000000000000000000000000000000000000000000000000000000000000240000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7000000000000000000000000f3e4872e6a4cf365888d93b6146a2baa7348f1a4000000000000000000000000000000000000000000000000000000002ea5b6fd000000000000000000000000000000000000000000000000afd1b70e521078af000000000000000000000000000000000000000000000000b01554c045e193eabe1b3db5335c4965a7a18ae85ecd14c0000000000000000000000000017933940000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000c000000000000000000000000068749665ff8d2d112fa859aa293f07a622782f38000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec700000000000000000000000000000000000000000000000000000000000001f480000000000000000000000068749665ff8d2d112fa859aa293f07a622782f38000000000000000000000000f3e4872e6a4cf365888d93b6146a2baa7348f1a400000000000000000000000000000000000000000000000000000000000027100000000000000000000000000000000000000000000000000000000000000000").expect("valid hex");
    let value: Option<&[u8]> = None;
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0x9c7c3eecdcf2b7debc8de3ddf2b9d95a75a9e7a8"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapExactAmountInOnUniswapV3 ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapExactAmountInOnUniswapV3 failed: {e}"),
    }
    assert!(result.is_ok(), "swapExactAmountInOnUniswapV3 should format successfully");
}

#[tokio::test]
async fn test_paraswap_v6_swapOnAugustusRFQTryBatchFill() {
    // Real tx: 0x7977345f821a968f537828737e05f7dbcaed85747f6de3df3b3584dafc614861
    let descriptor = load_descriptor();
    let calldata = hex::decode("da35bb0d000000000000000000000000000000000000000000000008048514524ed18b9400000000000000000000000000000000000000000000000008adbbb91b7ec858000000000000000000000000000000000000000000000000000000000000000006b761e722ba4af1bdb4f5ca529f3fc600000000000000000000000001793375000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e00000000000000000000000000000000000000000000000000000000000000360000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000203ba0b4ed3688a82ffe03dd8f25b6f5f1525f0074d53570ea2fb4cd9ee545b2960000000000000000000000000000000000000000000000000000000069c1339f000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000514910771af9ca656af840dff83e8264ecf986ca00000000000000000000000067336cec42645f55059eff241cb02ea5cc52ff860000000000000000000000006a000f20005980200259b80c510200304000106800000000000000000000000000000000000000000000000008c4779401c2f311000000000000000000000000000000000000000000000008190b87b987ed3d9d0000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000008190b87b987ed3d9d000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000002200000000000000000000000000000000000000000000000000000000000000041b562f381132b38af43b60d5d3141ede0dd479db8602a5fdd5b8c68ef8eff03bf5618a2ceaa138a5ea85d47f7fe0545b4eb6f2e912b85d3b21be62d5f8ed6a6121c00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").expect("valid hex");
    let value: Option<&[u8]> = None;
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0x25b6f5f1525f0074d53570ea2fb4cd9ee545b296"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapOnAugustusRFQTryBatchFill ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapOnAugustusRFQTryBatchFill failed: {e}"),
    }
    assert!(result.is_ok(), "swapOnAugustusRFQTryBatchFill should format successfully");
}

#[tokio::test]
async fn test_paraswap_v6_swapExactAmountIn_eth() {
    // Real tx: 0xacfdc46fed39a5984bff21d5ab8f9eccbf23eab76423a068527064043422b6b5
    let descriptor = load_descriptor();
    let calldata = hex::decode("e3ead59e00000000000000000000000000c600b30fb0400701010f4b080409018b9006e0000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7000000000000000000000000f3e4872e6a4cf365888d93b6146a2baa7348f1a4000000000000000000000000000000000000000000000000000000003b9aca00000000000000000000000000000000000000000000000000e0c2fe89619d4fc6000000000000000000000000000000000000000000000000e0fc97438f95033ceb03f5c26ddd4d24828db06d5b64e09e0000000000000000000000000179338b000000000000000000000000000000000000000000000000000000000000000008a3c2a819e3de7aca384c798269b3ce1cd0e43790000000000000000000000000000000000000000000000000000000000000000000000000000000000001600000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004c0000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004c000000000000000000000000000000460000000000000000000000000000027100000000000000000000000000000000000000000000002e0028400a4000a000b000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000280000000000000000000000000000000e0000000000000006c0000000000000af0d26f20001a72a18c002b00e6710000d68700ce00000000c000a50000ff00000300000000000000000000000000000000000000000000000000000000ce08024400000000000000000000000068749665ff8d2d112fa859aa293f07a622782f38000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec700000000000000000000000000000000000000000020c49ba5e353f88000137c00000000000000000000000000000000000000000000000010b076008000000000000000000000000000000000c600b30fb0400701010f4b080409018b9006e00000000000000000000000000000016000000000000001200000000000001c20e592427a0aece92de3edee1f18e0157c0586156400000140008400000000000300000000000000000000000000000000000000000000000000000000c04b8d59000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000c600b30fb0400701010f4b080409018b9006e00000000000000000000000000000000000000000000000000000000069ca6eb1000000000000000000000000000000000000000000000000000000002aea54000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000002bdac17f958d2ee523a2206206994597c13d831ec70001f468749665ff8d2d112fa859aa293f07a622782f38000000000000000000000000000000000000000000e592427a0aece92de3edee1f18e0157c0586156400000140008400000000000300000000000000000000000000000000000000000000000000000000c04b8d59000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000006a000f20005980200259b80c51020030400010680000000000000000000000000000000000000000000000000000000069ca6eb100000000000000000000000000000000000000000000000000000000000376540000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000002b68749665ff8d2d112fa859aa293f07a622782f38002710f3e4872e6a4cf365888d93b6146a2baa7348f1a4000000000000000000000000000000000000000000").expect("valid hex");
    let value: Option<&[u8]> = None;
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0x227024f48ccdc738245b40d0b4ffffef85d77881"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapExactAmountIn_eth ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapExactAmountIn_eth failed: {e}"),
    }
    assert!(result.is_ok(), "swapExactAmountIn_eth should format successfully");
}

#[tokio::test]
async fn test_paraswap_v6_swapExactAmountInOnUniswapV2_b() {
    // Real tx: 0x06480373e07dc9f0f6434384942023e2efc0c6649fc060547bdb86ffa324c6ac
    let descriptor = load_descriptor();
    let calldata = hex::decode("e8bb3b6c0000000000000000000000000000000000000000000000000000000000000060cfd59c0f530db36eea8ccbfe744f01fe3556925e1800000000000000000000430000000000000000000000000000000000000000000000000000000000000200000000000000000000000000615987d46003cc37387dbe544ff4f16fa1200077000000000000000000000000fefe157c9d0ae025213092ff9a5cb56ab492bab80000000000000000000000000000000000000000000000000000763bfbd2200000000000000000000000000000000000000000000000000000006385b9a5fc9300000000000000000000000000000000000000000000000000006405c10371a0cff2c90438204226a2a0ba0ec8aaa18d00000000000000000000000001793325000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000080615987d46003cc37387dbe544ff4f16fa1200077c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000000000000000000000000001c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2fefe157c9d0ae025213092ff9a5cb56ab492bab80000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000").expect("valid hex");
    let value: Option<&[u8]> = None;
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0x6386ce3b32baf79a7c46d278e1d1a64a8a7554c1"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapExactAmountInOnUniswapV2_b ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapExactAmountInOnUniswapV2_b failed: {e}"),
    }
    assert!(result.is_ok(), "swapExactAmountInOnUniswapV2_b should format successfully");
}

#[tokio::test]
async fn test_paraswap_v6_swapOnAugustusRFQTryBatchFill_b() {
    // Real tx: 0x598c406c2460b366280dc5ef2470fff7ebd762229ddbcd9641eabf7257d31109
    let descriptor = load_descriptor();
    let calldata = hex::decode("da35bb0d00000000000000000000000000000000000000000000003635c9adc5dea000000000000000000000000000000000000000000000000000003ab8239f624f537800000000000000000000000000000000000000000000000000000000000000009a3a6797d0b34c70b77689693eb5e64900000000000000000000000001793361000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e00000000000000000000000000000000000000000000000000000000000000360000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000208aa4c777b32626b74592681b25b6f5f1525f0074d53570ea2fb4cd9ee545b2960000000000000000000000000000000000000000000000000000000069c132af000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000514910771af9ca656af840dff83e8264ecf986ca00000000000000000000000067336cec42645f55059eff241cb02ea5cc52ff860000000000000000000000006a000f20005980200259b80c51020030400010680000000000000000000000000000000000000000000000003b4fd5f4c65a69e9000000000000000000000000000000000000000000000036c090d0ca688800000000000000000000000000000000000000000000000000000000000000000180000000000000000000000000000000000000000000000036c090d0ca68880000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000002200000000000000000000000000000000000000000000000000000000000000041bdd429358705d82035d4dc33d23ce13ab412639d8b2ad1c689cd041d60c0bdc4506c2642cd9c33757a546cc0b412df86b1c0e56d3b96da1b6826ebb7746072ff1b00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").expect("valid hex");
    let value: Option<&[u8]> = None;
    let tx = TransactionContext {
        chain_id: 1,
        to: "0x6a000f20005980200259b80c5102003040001068",
        calldata: &calldata,
        value,
        from: Some("0x25b6f5f1525f0074d53570ea2fb4cd9ee545b296"),
        implementation_address: None,
    };
    let provider = token_source();
    let descriptors = wrap_rd(descriptor, 1, "0x6a000f20005980200259b80c5102003040001068");
    let result = format_calldata(&descriptors, &tx, &provider).await;
    match &result {
        Ok(model) => {
            eprintln!("=== swapOnAugustusRFQTryBatchFill_b ===");
            eprintln!("  intent: {:?}", model.intent);
            eprintln!("  interpolated: {:?}", model.interpolated_intent);
            for entry in &model.entries {
                match entry {
                    DisplayEntry::Item(item) => {
                        eprintln!("  [{}]: {}", item.label, item.value);
                    }
                    DisplayEntry::Group { label, items, .. } => {
                        eprintln!("  group: {}", label);
                        for item in items {
                            eprintln!("    [{}]: {}", item.label, item.value);
                        }
                    }
                    _ => eprintln!("  other entry"),
                }
            }
            if !model.warnings.is_empty() {
                eprintln!("  warnings: {:?}", model.warnings);
            }
        }
        Err(e) => panic!("swapOnAugustusRFQTryBatchFill_b failed: {e}"),
    }
    assert!(result.is_ok(), "swapOnAugustusRFQTryBatchFill_b should format successfully");
}
