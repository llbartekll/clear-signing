//! Integration tests for Morpho Blue lending protocol using real on-chain transactions.

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{
    CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource,
};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, DisplayEntry, DisplayModel, TransactionContext};

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

const MORPHO_ADDR: &str = "0xbbbbbbbbbb9cc5e90e3b3af64bdaf62c37eeffcb";

fn morpho_token_source() -> CompositeDataProvider {
    let mut custom = StaticTokenSource::new();
    let tokens = [
        (
            "0x3b855aa8cc56a3cbd5dbb5456f5a13ce86aa0fe8",
            "stakedao-FrxMsUSD",
            18,
            "Stake DAO frxUSD/msUSD",
        ),
        (
            "0x80e1048ede66ec4c364b4f22c8768fc657ff6a42",
            "upUSDC",
            6,
            "Upshift USDC",
        ),
        (
            "0xbf5495efe5db9ce00f80364c8b423567e58d2110",
            "EZETH",
            18,
            "Renzo Restaked ETH",
        ),
        (
            "0xc26a6fa2c37b38e549a4a1807543801db684f99c",
            "AA_FalconXUSDC",
            18,
            "Pareto AA Tranche - FalconXUSDC",
        ),
        (
            "0xcacd6fd266af91b8aed52accc382b4e165586e29",
            "frxUSD",
            18,
            "Frax USD",
        ),
        (
            "0xdc035d45d973e3ec169d2276ddab16f1e407384f",
            "USDS",
            18,
            "USDS",
        ),
        (
            "0xe0f63a424a4439cbe457d80e4f4b51ad25b2c56c",
            "SPX",
            8,
            "SPX6900",
        ),
        // USDC needed for supply test
        (
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "USDC",
            6,
            "USD Coin",
        ),
        // WETH needed for withdraw_collateral test
        (
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
            "WETH",
            18,
            "Wrapped Ether",
        ),
    ];
    for (addr, symbol, decimals, name) in tokens {
        custom.insert(
            1,
            addr,
            TokenMeta {
                symbol: symbol.to_string(),
                decimals,
                name: name.to_string(),
            },
        );
    }
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

async fn run_morpho_test(calldata_hex: &str, from: &str) -> DisplayModel {
    let descriptor = load_descriptor("calldata-MorphoBlue.json");
    let descriptors = wrap_rd(descriptor, 1, MORPHO_ADDR);
    let calldata = decode_hex(calldata_hex);
    let provider = morpho_token_source();
    let tx = TransactionContext {
        chain_id: 1,
        to: MORPHO_ADDR,
        calldata: &calldata,
        value: None,
        from: Some(from),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &provider).await.unwrap();
    assert!(result.is_clear_signed(), "expected clear-signed outcome");
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
    result.into_model()
}

#[tokio::test]
async fn morpho_borrow() {
    let result = run_morpho_test(
        "0x50d8cd4b000000000000000000000000dc035d45d973e3ec169d2276ddab16f1e407384f000000000000000000000000e0f63a424a4439cbe457d80e4f4b51ad25b2c56c000000000000000000000000da63266b5184d08dbfbace96267837c45d7d34da000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc00000000000000000000000000000000000000000000000008ac7230489e800000000000000000000000000000000000000000000000000340aad21b3b7000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000f11cc888d8da84d3bf735b7b357814fd071e8b1a000000000000000000000000f11cc888d8da84d3bf735b7b357814fd071e8b1a",
        "0xf11cc888d8da84d3bf735b7b357814fd071e8b1a",
    ).await;

    assert_eq!(result.intent, "Borrow from Morpho Market");
    assert_eq!(result.owner.as_deref(), Some("Morpho DAO"));
    assert_eq!(
        get_entry_value(&result, "Loan Token"),
        "0xdC035D45d973E3EC169d2276DDab16f1e407384F"
    );
    assert_eq!(
        get_entry_value(&result, "Collateral Token"),
        "0xE0f63A424a4439cBE457D80E4f4b51aD25b2c56C"
    );
    assert_eq!(
        get_entry_value(&result, "On Behalf"),
        "0xf11Cc888d8Da84D3bF735b7b357814fD071e8B1A"
    );
    assert_eq!(
        get_entry_value(&result, "Receiver"),
        "0xf11Cc888d8Da84D3bF735b7b357814fD071e8B1A"
    );
}

#[tokio::test]
async fn morpho_repay() {
    let result = run_morpho_test(
        "0x20b76e81000000000000000000000000cacd6fd266af91b8aed52accc382b4e165586e290000000000000000000000003b855aa8cc56a3cbd5dbb5456f5a13ce86aa0fe8000000000000000000000000c5860e9e6b6f6e9d79dce5c5ab0f7a4b878bd431000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc0000000000000000000000000000000000000000000000000d1d507e40be80000000000000000000000000000000000000000000000000018c1018ad7bfc118b0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000908137923a9dd0bcb1373ffeaef5f29fc069b27200000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000000",
        "0x908137923a9dd0bcb1373ffeaef5f29fc069b272",
    ).await;

    assert_eq!(result.intent, "Repay on Morpho Market");
    assert_eq!(result.owner.as_deref(), Some("Morpho DAO"));
    assert_eq!(
        get_entry_value(&result, "Loan Token"),
        "0xCAcd6fd266aF91b8AeD52aCCc382b4e165586E29"
    );
    assert_eq!(
        get_entry_value(&result, "Collateral Token"),
        "0x3B855AA8CC56a3cBd5dBb5456F5A13Ce86AA0fe8"
    );
    assert_eq!(get_entry_value(&result, "Data"), "0x");
}

#[tokio::test]
async fn morpho_supply() {
    let result = run_morpho_test(
        "0xa99aad89000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48000000000000000000000000c26a6fa2c37b38e549a4a1807543801db684f99c00000000000000000000000052ea2c12734b5bb61e1edf52bb0f01d9206493fc000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc0000000000000000000000000000000000000000000000000aaf96eb9d0d0000000000000000000000000000000000000000000000000000000000e8d4a51000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000084d1b180d67ba40a4cb7aeb78af6a8bf80fc5c6300000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000000000000000000000",
        "0x84d1b180d67ba40a4cb7aeb78af6a8bf80fc5c63",
    ).await;

    assert_eq!(result.intent, "Supply on Morpho Market");
    assert_eq!(result.owner.as_deref(), Some("Morpho DAO"));
    assert_eq!(get_entry_value(&result, "Assets"), "1000000000000");
    assert_eq!(
        get_entry_value(&result, "On Behalf"),
        "0x84D1b180d67Ba40A4Cb7AEb78AF6a8BF80fC5C63"
    );
}

#[tokio::test]
async fn morpho_supply_collateral() {
    let result = run_morpho_test(
        "0x238d6579000000000000000000000000dc035d45d973e3ec169d2276ddab16f1e407384f000000000000000000000000e0f63a424a4439cbe457d80e4f4b51ad25b2c56c000000000000000000000000da63266b5184d08dbfbace96267837c45d7d34da000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc00000000000000000000000000000000000000000000000008ac7230489e800000000000000000000000000000000000000000000000000000000004bf6470e7000000000000000000000000f11cc888d8da84d3bf735b7b357814fd071e8b1a00000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000",
        "0xf11cc888d8da84d3bf735b7b357814fd071e8b1a",
    ).await;

    assert_eq!(result.intent, "Supply Collateral on Morpho Market");
    assert_eq!(result.owner.as_deref(), Some("Morpho DAO"));
    assert_eq!(
        get_entry_value(&result, "Loan Token"),
        "0xdC035D45d973E3EC169d2276DDab16f1e407384F"
    );
    assert_eq!(get_entry_value(&result, "Assets"), "20390899943");
}

#[tokio::test]
async fn morpho_withdraw() {
    let result = run_morpho_test(
        "0x5c2bea49000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb4800000000000000000000000080e1048ede66ec4c364b4f22c8768fc657ff6a42000000000000000000000000f2c9ee3fdc5d1e360d51b4840b4096f63913df93000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc0000000000000000000000000000000000000000000000000cb2bba6f17b8000000000000000000000000000000000000000000000000000000000008f0d180000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008fee409f8f772667add2a2ccfb8c5182a7349ce90000000000000000000000008fee409f8f772667add2a2ccfb8c5182a7349ce9",
        "0x8fee409f8f772667add2a2ccfb8c5182a7349ce9",
    ).await;

    assert_eq!(result.intent, "Withdraw from Morpho Market");
    assert_eq!(result.owner.as_deref(), Some("Morpho DAO"));
    assert_eq!(get_entry_value(&result, "Assets"), "2400000000");
    assert_eq!(
        get_entry_value(&result, "On Behalf"),
        "0x8fee409f8F772667ADD2a2ccfB8C5182a7349cE9"
    );
    assert_eq!(
        get_entry_value(&result, "Receiver"),
        "0x8fee409f8F772667ADD2a2ccfB8C5182a7349cE9"
    );
}

#[tokio::test]
async fn morpho_withdraw_collateral() {
    let result = run_morpho_test(
        "0x8720316d000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000bf5495efe5db9ce00f80364c8b423567e58d211000000000000000000000000094f93f1eadb8a2f73c415ad4c19cb791e6d0192b000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc0000000000000000000000000000000000000000000000000d1d507e40be80000000000000000000000000000000000000000000000000000b6cd31d327d7bfd000000000000000000000000191e9eecb83861eaa758f0c7c31aaacb5eac87ab000000000000000000000000191e9eecb83861eaa758f0c7c31aaacb5eac87ab",
        "0x191e9eecb83861eaa758f0c7c31aaacb5eac87ab",
    ).await;

    assert_eq!(result.intent, "Withdraw Collateral from Morpho Market");
    assert_eq!(result.owner.as_deref(), Some("Morpho DAO"));
    assert_eq!(
        get_entry_value(&result, "Loan Token"),
        "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
    );
    assert_eq!(
        get_entry_value(&result, "Collateral Token"),
        "0xbf5495Efe5DB9ce00f80364C8B423567e58d2110"
    );
    assert_eq!(get_entry_value(&result, "Assets"), "823264954256555005");
}
