//! Integration tests for QuickSwap (Polygon) using real on-chain transactions.
//! Covers swap variants, liquidity operations, fee-on-transfer, permit signatures.

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{CompositeDataProvider, StaticTokenSource, TokenMeta, WellKnownTokenSource};
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

const QUICKSWAP_ADDR: &str = "0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff";

fn quickswap_token_source() -> CompositeDataProvider {
    let mut custom = StaticTokenSource::new();
    let tokens: &[(&str, &str, u8, &str)] = &[
        (
            "0x2791bca1f2de4661ed88a30c99a7a9449aa84174",
            "USDC.e",
            6,
            "USD Coin (PoS)",
        ),
        (
            "0x7ceb23fd6bc0add59e62ac25578270cff1b9f619",
            "WETH",
            18,
            "Wrapped Ether",
        ),
        (
            "0xc2132d05d31c914a87c6611c10748aeb04b58e8f",
            "USDT",
            6,
            "Tether USD",
        ),
        (
            "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270",
            "WPOL",
            18,
            "Wrapped Polygon Ecosystem Token",
        ),
        (
            "0x013e90c6ea4ea50625c3ff1288dbd154f8c2479b",
            "IWT",
            18,
            "Intero Web3 Token",
        ),
        (
            "0x0c9c7712c83b3c70e7c5e11100d33d9401bdf9dd",
            "WOMBAT",
            18,
            "Wombat",
        ),
        (
            "0x146e58d34eab0bff7e0a63cfe9332908d680c667",
            "PDDOLLAR",
            18,
            "pddollar",
        ),
        (
            "0x173f9c8492f19ebe9987e8ea6507e17d40f23328",
            "TDOX",
            9,
            "TDOX",
        ),
        (
            "0x282d8efce846a88b159800bd4130ad77443fa1a1",
            "mOCEAN",
            18,
            "Ocean Token (PoS)",
        ),
        (
            "0x2ed945dc703d85c80225d95abde41cdee14e1992",
            "SAGE",
            18,
            "PolySage",
        ),
        (
            "0x431d5dff03120afa4bdf332c61a6e1766ef37bdb",
            "JPYC",
            18,
            "JPY Coin",
        ),
        (
            "0x56f4d3019680b992e2e18533385293710b44010d",
            "CD",
            18,
            "Credit DAO Token",
        ),
        (
            "0x692597b009d13c4049a947cab2239b7d6517875f",
            "UST",
            18,
            "Wrapped UST Token (PoS)",
        ),
        (
            "0xb5c064f955d8e7f38fe0460c556a72987494ee17",
            "QUICK",
            18,
            "QuickSwap",
        ),
        (
            "0xf1428850f92b87e629c6f3a3b75bffbc496f7ba6",
            "GEO$",
            18,
            "GEOPOLY",
        ),
        (
            "0xffb9f1907f827709b0ed09b37956cd3c7462abdb",
            "DUCKIES",
            2,
            "Yellow Duckies",
        ),
    ];
    for (addr, symbol, decimals, name) in tokens {
        custom.insert(
            137,
            addr,
            TokenMeta {
                symbol: symbol.to_string(),
                decimals: *decimals,
                name: name.to_string(),
            },
        );
    }
    CompositeDataProvider::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
}

async fn run_quickswap_test(calldata_hex: &str, value_hex: &str, from: &str) -> DisplayModel {
    let descriptor = load_descriptor("calldata-QuickSwap.json");
    let descriptors = wrap_rd(descriptor, 137, QUICKSWAP_ADDR);
    let calldata = decode_hex(calldata_hex);
    let val = value_bytes(value_hex);
    let provider = quickswap_token_source();
    let tx = TransactionContext {
        chain_id: 137,
        to: QUICKSWAP_ADDR,
        calldata: &calldata,
        value: val.as_deref(),
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
async fn quickswap_swap_exact_tokens_for_tokens() {
    // mOCEAN -> WPOL -> USDC.e multi-hop swap
    let result = run_quickswap_test(
        "0x38ed17390000000000000000000000000000000000000000000000008ac7230489e8000000000000000000000000000000000000000000000000000000000000003cbb8500000000000000000000000000000000000000000000000000000000000000a0000000000000000000000000a7112994f478dbf1e8829622b46604ac324bb58b00000000000000000000000000000000000000000000000000000000c80999e90000000000000000000000000000000000000000000000000000000000000003000000000000000000000000282d8efce846a88b159800bd4130ad77443fa1a10000000000000000000000000d500b1d8e8ef31e21c99d1db9a6444d3adf12700000000000000000000000002791bca1f2de4661ed88a30c99a7a9449aa84174",
        "0x0",
        "0xa7112994f478dbf1e8829622b46604ac324bb58b",
    ).await;

    assert_eq!(result.intent, "Swap");
    assert_eq!(result.owner.as_deref(), Some("QuickSwap"));
    assert_eq!(get_entry_value(&result, "Amount to Send"), "10 mOCEAN");
    assert_eq!(
        get_entry_value(&result, "Minimum to Receive"),
        "3.980165 USDC.e"
    );
    assert_eq!(
        get_entry_value(&result, "Beneficiary"),
        "0xa7112994F478DbF1e8829622b46604AC324bB58B"
    );
}

#[tokio::test]
async fn quickswap_swap_exact_eth_for_tokens() {
    // Native MATIC -> USDT swap with value
    let result = run_quickswap_test(
        "0x7ff36ab5000000000000000000000000000000000000000000000000000000000749b4d10000000000000000000000000000000000000000000000000000000000000080000000000000000000000000fe3de3b49d1de7eb0cb28b57ac2abfdb3f1118dd00000000000000000000000000000000000000000000000000000000c809982100000000000000000000000000000000000000000000000000000000000000020000000000000000000000000d500b1d8e8ef31e21c99d1db9a6444d3adf1270000000000000000000000000c2132d05d31c914a87c6611c10748aeb04b58e8f",
        "0x05dacd13ca9e300000",
        "0xfe3de3b49d1de7eb0cb28b57ac2abfdb3f1118dd",
    ).await;

    assert_eq!(result.intent, "Swap");
    assert_eq!(get_entry_value(&result, "Amount to Send"), "108 MATIC");
    assert_eq!(
        get_entry_value(&result, "Minimum to Receive"),
        "122.270929 USDT"
    );
}

#[tokio::test]
async fn quickswap_swap_tokens_for_exact_tokens() {
    // Exact-output swap: USDT -> GEO$ (reversed amount semantics)
    let result = run_quickswap_test(
        "0x8803dbee000000000000000000000000000000000000000000001403ff0861aaf200000000000000000000000000000000000000000000000000000000000000045310a600000000000000000000000000000000000000000000000000000000000000a000000000000000000000000064d11aa7f7eeaa52a19e68dc63188866e64c8d72000000000000000000000000000000000000000000000000000000006404cc1f0000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c2132d05d31c914a87c6611c10748aeb04b58e8f000000000000000000000000f1428850f92b87e629c6f3a3b75bffbc496f7ba6",
        "0x0",
        "0x64d11aa7f7eeaa52a19e68dc63188866e64c8d72",
    ).await;

    assert_eq!(result.intent, "Swap");
    assert_eq!(
        get_entry_value(&result, "Amount to Receive"),
        "94521.04693528035065856 GEO$"
    );
    assert_eq!(
        get_entry_value(&result, "Maximum to Send"),
        "72.552614 USDT"
    );
}

#[tokio::test]
async fn quickswap_swap_exact_tokens_for_tokens_supporting_fee() {
    // Fee-on-transfer variant: CD -> WPOL -> WETH -> USDT (4-hop)
    let result = run_quickswap_test(
        "0x5c11d7950000000000000000000000000000000000000000000000000759891e8b626ad500000000000000000000000000000000000000000000000000000000000c436300000000000000000000000000000000000000000000000000000000000000a0000000000000000000000000d682a75b581aa237b5a60adaefd263ded0065140000000000000000000000000000000000000000000000000000000006404cceb000000000000000000000000000000000000000000000000000000000000000400000000000000000000000056f4d3019680b992e2e18533385293710b44010d0000000000000000000000000d500b1d8e8ef31e21c99d1db9a6444d3adf12700000000000000000000000007ceb23fd6bc0add59e62ac25578270cff1b9f619000000000000000000000000c2132d05d31c914a87c6611c10748aeb04b58e8f",
        "0x0",
        "0xd682a75b581aa237b5a60adaefd263ded0065140",
    ).await;

    assert_eq!(result.intent, "Swap");
    assert_eq!(
        get_entry_value(&result, "Amount to Send"),
        "0.529605195473251029 CD"
    );
    assert_eq!(
        get_entry_value(&result, "Minimum to Receive"),
        "0.803683 USDT"
    );
}

#[tokio::test]
async fn quickswap_add_liquidity() {
    // Add WOMBAT + USDC.e liquidity
    let result = run_quickswap_test(
        "0xe8e337000000000000000000000000000c9c7712c83b3c70e7c5e11100d33d9401bdf9dd0000000000000000000000002791bca1f2de4661ed88a30c99a7a9449aa84174000000000000000000000000000000000000000000000036fde9d4083ce78f8100000000000000000000000000000000000000000000000000000000002cbfe1000000000000000000000000000000000000000000000036efd5e1e6700b2b4300000000000000000000000000000000000000000000000000000000002cb46c00000000000000000000000045c75f061688a463c6ce14b388688e61ebd0d7e5000000000000000000000000000000000000000000000000000000006404ce14",
        "0x0",
        "0x45c75f061688a463c6ce14b388688e61ebd0d7e5",
    ).await;

    assert_eq!(result.intent, "Add Liquidity");
    assert_eq!(result.owner.as_deref(), Some("QuickSwap"));
    // Check both token amounts
    let desired_entries: Vec<_> = result
        .entries
        .iter()
        .filter_map(|e| {
            if let DisplayEntry::Item(item) = e {
                if item.label == "Desired amount" {
                    return Some(item.value.clone());
                }
            }
            None
        })
        .collect();
    assert_eq!(desired_entries.len(), 2);
    assert!(desired_entries.contains(&"1014.420568073331773313 WOMBAT".to_string()));
    assert!(desired_entries.contains(&"2.932705 USDC.e".to_string()));
}

#[tokio::test]
async fn quickswap_remove_liquidity_eth_with_permit() {
    // Remove SAGE liquidity with permit signature
    let result = run_quickswap_test(
        "0xded9382a0000000000000000000000002ed945dc703d85c80225d95abde41cdee14e1992000000000000000000000000000000000000000000000000022ad0cb72be71a3000000000000000000000000000000000000000000000000001c749c760b197d0000000000000000000000000000000000000000000000002d6b23773a55183e0000000000000000000000003272d5c631096d013fc40925e9acb6a7faf70866000000000000000000000000000000000000000000000000000000006160cbf90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001c79f7147393115080f047bad60bf6d4dfdcb74fb938913b4530fe17d937f244362383de7466318312224315d3e92e4fe74f2b791023165b75b039d1292b4dd919",
        "0x0",
        "0x3272d5c631096d013fc40925e9acb6a7faf70866",
    ).await;

    assert_eq!(result.intent, "Remove Liquidity");
    assert_eq!(result.owner.as_deref(), Some("QuickSwap"));
    // Verify minimum amount for token we can resolve
    let min_entries: Vec<_> = result
        .entries
        .iter()
        .filter_map(|e| {
            if let DisplayEntry::Item(item) = e {
                if item.label == "Minimum amount" {
                    return Some(item.value.clone());
                }
            }
            None
        })
        .collect();
    assert_eq!(min_entries.len(), 2);
    assert!(min_entries.contains(&"0.008009514692057469 SAGE".to_string()));
    assert_eq!(
        get_entry_value(&result, "Beneficiary"),
        "0x3272D5C631096d013fc40925E9AcB6A7faF70866"
    );
}

#[tokio::test]
async fn quickswap_add_liquidity_eth() {
    // Add PDDOLLAR + native MATIC liquidity with value
    let result = run_quickswap_test(
        "0xf305d719000000000000000000000000146e58d34eab0bff7e0a63cfe9332908d680c66700000000000000000000000000000000000001516b4d1f2b1d156a60b6f7412a000000000000000000000000000000000000014fbb679dbb89bcfd0bc04a7f8e0000000000000000000000000000000000000000000000001b9de674df0700000000000000000000000000008a2f7f99b3b20cbe8343ac9f4292c9a1b12a0131000000000000000000000000000000000000000000000000000000006404ce44",
        "0x1bc16d674ec80000",
        "0x8a2f7f99b3b20cbe8343ac9f4292c9a1b12a0131",
    ).await;

    assert_eq!(result.intent, "Add Liquidity");
    assert_eq!(result.owner.as_deref(), Some("QuickSwap"));
    let desired_entries: Vec<_> = result
        .entries
        .iter()
        .filter_map(|e| {
            if let DisplayEntry::Item(item) = e {
                if item.label == "Desired amount" {
                    return Some(item.value.clone());
                }
            }
            None
        })
        .collect();
    assert!(desired_entries.contains(&"26733098897834.742680118057451818 PDDOLLAR".to_string()));
}
