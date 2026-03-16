//! Integration tests for wallet-side batch operations (wallet_sendCalls / EIP-5792).
//!
//! Per the ERC-7730 spec, batch operations are handled by the **wallet**, not the library.
//! The wallet calls `format_calldata()` once per inner call and composes the results.
//! These tests document and validate the expected wallet-side batch usage pattern.

use erc7730::decoder::parse_signature;
use erc7730::resolver::ResolvedDescriptor;
use erc7730::token::{EmptyTokenSource, StaticTokenSource, TokenMeta};
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_calldata, format_calldata_multi, DisplayEntry, DisplayModel};

fn load_descriptor(fixture: &str) -> Descriptor {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    Descriptor::from_json(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn address_word(hex_addr: &str) -> Vec<u8> {
    let hex_str = hex_addr
        .strip_prefix("0x")
        .or_else(|| hex_addr.strip_prefix("0X"))
        .unwrap_or(hex_addr);
    let addr_bytes = hex::decode(hex_str).expect("valid hex address");
    let mut word = vec![0u8; 12];
    word.extend_from_slice(&addr_bytes);
    assert_eq!(word.len(), 32);
    word
}

fn uint_word(val: u128) -> Vec<u8> {
    let mut word = vec![0u8; 16];
    word.extend_from_slice(&val.to_be_bytes());
    assert_eq!(word.len(), 32);
    word
}

fn build_erc20_transfer_calldata(to: &str, amount: u128) -> Vec<u8> {
    let sig = parse_signature("transfer(address,uint256)").unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(to));
    calldata.extend_from_slice(&uint_word(amount));
    calldata
}

fn build_erc20_approve_calldata(spender: &str, amount: u128) -> Vec<u8> {
    let sig = parse_signature("approve(address,uint256)").unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(spender));
    calldata.extend_from_slice(&uint_word(amount));
    calldata
}

/// Simulate the wallet-side batch intent join pattern.
/// Uses `interpolated_intent` when available, falls back to `intent`.
fn join_intents(models: &[&DisplayModel]) -> String {
    models
        .iter()
        .map(|m| {
            m.interpolated_intent
                .as_deref()
                .unwrap_or(&m.intent)
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

fn pad32(len: usize) -> usize {
    len.div_ceil(32) * 32
}

fn build_exec_transaction_calldata(
    to: &str,
    value: u128,
    inner_calldata: &[u8],
    operation: u8,
) -> Vec<u8> {
    let sig = parse_signature(
        "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)",
    )
    .unwrap();

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(to));
    calldata.extend_from_slice(&uint_word(value));
    calldata.extend_from_slice(&uint_word(320)); // data offset
    calldata.extend_from_slice(&uint_word(operation as u128));
    calldata.extend_from_slice(&uint_word(0)); // safeTxGas
    calldata.extend_from_slice(&uint_word(21000)); // baseGas
    calldata.extend_from_slice(&uint_word(0)); // gasPrice
    calldata.extend_from_slice(&[0u8; 32]); // gasToken
    calldata.extend_from_slice(&[0u8; 32]); // refundReceiver
    let data_offset = 320 + 32 + pad32(inner_calldata.len());
    calldata.extend_from_slice(&uint_word(data_offset as u128)); // signatures offset

    calldata.extend_from_slice(&uint_word(inner_calldata.len() as u128));
    calldata.extend_from_slice(inner_calldata);
    let padding = pad32(inner_calldata.len()) - inner_calldata.len();
    calldata.extend_from_slice(&vec![0u8; padding]);

    calldata.extend_from_slice(&uint_word(0)); // signatures length = 0

    calldata
}

/// Wallet calls `format_calldata()` twice for two ERC-20 transfers to different
/// recipients. Verifies each produces a correct `DisplayModel` with intent and
/// formatted amounts. Joins intents with " and ".
#[test]
fn wallet_batch_two_erc20_transfers() {
    let descriptor = load_descriptor("erc20-transfer.json");
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient_a = "0x1111111111111111111111111111111111111111";
    let recipient_b = "0x2222222222222222222222222222222222222222";

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        1,
        usdc_addr,
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );

    // Wallet calls format_calldata once per inner call
    let calldata_a = build_erc20_transfer_calldata(recipient_a, 1_000_000); // 1 USDC
    let result_a =
        format_calldata(&descriptor, 1, usdc_addr, &calldata_a, None, &tokens).unwrap();

    let calldata_b = build_erc20_transfer_calldata(recipient_b, 5_000_000); // 5 USDC
    let result_b =
        format_calldata(&descriptor, 1, usdc_addr, &calldata_b, None, &tokens).unwrap();

    // Each produces correct DisplayModel
    assert_eq!(result_a.intent, "Transfer tokens");
    assert_eq!(result_b.intent, "Transfer tokens");

    if let DisplayEntry::Item(ref item) = result_a.entries[1] {
        assert_eq!(item.label, "Amount");
        assert_eq!(item.value, "1 USDC");
    } else {
        panic!("expected Item for amount A");
    }

    if let DisplayEntry::Item(ref item) = result_b.entries[1] {
        assert_eq!(item.label, "Amount");
        assert_eq!(item.value, "5 USDC");
    } else {
        panic!("expected Item for amount B");
    }

    // Wallet joins intents with " and "
    let batch_summary = join_intents(&[&result_a, &result_b]);
    assert_eq!(batch_summary, "Transfer tokens and Transfer tokens");
}

/// Wallet calls `format_calldata()` for a known contract (ERC-20 transfer) and an
/// unknown contract. Known call produces full formatting, unknown call degrades
/// gracefully to raw preview.
#[test]
fn wallet_batch_mixed_known_unknown() {
    let descriptor = load_descriptor("erc20-transfer.json");
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let unknown_addr = "0x0000000000000000000000000000000000000042";
    let recipient = "0x1234567890123456789012345678901234567890";

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        1,
        usdc_addr,
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );

    // Known call: ERC-20 transfer — full formatting
    let known_calldata = build_erc20_transfer_calldata(recipient, 2_000_000);
    let known_result =
        format_calldata(&descriptor, 1, usdc_addr, &known_calldata, None, &tokens).unwrap();

    // Unknown call: random selector not in descriptor — graceful degradation
    let unknown_calldata =
        hex::decode("deadbeef000000000000000000000000000000000000000000000000000000000000002a")
            .unwrap();
    let unknown_result = format_calldata(
        &descriptor,
        1,
        unknown_addr,
        &unknown_calldata,
        None,
        &EmptyTokenSource,
    )
    .unwrap();

    // Known call: full formatting with intent and token amounts
    assert_eq!(known_result.intent, "Transfer tokens");
    assert!(known_result.warnings.is_empty());
    if let DisplayEntry::Item(ref item) = known_result.entries[1] {
        assert_eq!(item.value, "2 USDC");
    } else {
        panic!("expected Item for known amount");
    }

    // Unknown call: raw fallback with warning
    assert!(
        unknown_result.intent.contains("Unknown function"),
        "expected raw fallback intent, got: {}",
        unknown_result.intent
    );
    assert!(
        !unknown_result.warnings.is_empty(),
        "expected warnings for unknown selector"
    );

    // Wallet can still compose both into a batch summary
    let batch_summary = join_intents(&[&known_result, &unknown_result]);
    assert!(batch_summary.contains("Transfer tokens"));
    assert!(batch_summary.contains("Unknown function"));
}

/// Wallet calls `format_calldata()` for 3 calls (approve + transfer + deposit).
/// Verifies `interpolated_intent` concatenation with " and " separator matches
/// the spec expectation.
#[test]
fn wallet_batch_intent_concatenation() {
    let approve_descriptor = load_descriptor("erc20-approve.json");
    let transfer_descriptor = load_descriptor("erc20-transfer.json");

    // Inline descriptor for deposit(uint256)
    let deposit_descriptor = Descriptor::from_json(
        r#"{
        "context": {
            "contract": {
                "deployments": [
                    { "chainId": 1, "address": "0x7d2768de32b0b80b7a3454c06bdac94a69ddc7a9" }
                ]
            }
        },
        "metadata": {
            "owner": "Aave",
            "contractName": "Lending Pool",
            "enums": {},
            "constants": {},
            "addressBook": {},
            "maps": {}
        },
        "display": {
            "definitions": {},
            "formats": {
                "deposit(uint256 amount)": {
                    "intent": "Deposit funds",
                    "fields": [
                        {
                            "path": "amount",
                            "label": "Amount",
                            "format": "raw"
                        }
                    ]
                }
            }
        }
    }"#,
    )
    .unwrap();

    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let deposit_addr = "0x7d2768de32b0b80b7a3454c06bdac94a69ddc7a9";
    let spender = "0xdef1c0ded9bec7f1a1670819833240f027b25eff";

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        1,
        usdc_addr,
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );

    // Call 1: approve
    let approve_calldata = build_erc20_approve_calldata(spender, 10_000_000);
    let approve_result =
        format_calldata(&approve_descriptor, 1, usdc_addr, &approve_calldata, None, &tokens)
            .unwrap();

    // Call 2: transfer
    let recipient = "0x3333333333333333333333333333333333333333";
    let transfer_calldata = build_erc20_transfer_calldata(recipient, 10_000_000);
    let transfer_result =
        format_calldata(&transfer_descriptor, 1, usdc_addr, &transfer_calldata, None, &tokens)
            .unwrap();

    // Call 3: deposit
    let deposit_sig = parse_signature("deposit(uint256)").unwrap();
    let mut deposit_calldata = Vec::new();
    deposit_calldata.extend_from_slice(&deposit_sig.selector);
    deposit_calldata.extend_from_slice(&uint_word(5_000_000));
    let deposit_result = format_calldata(
        &deposit_descriptor,
        1,
        deposit_addr,
        &deposit_calldata,
        None,
        &EmptyTokenSource,
    )
    .unwrap();

    // Verify individual intents
    assert_eq!(approve_result.intent, "Approve token spending");
    assert_eq!(transfer_result.intent, "Transfer tokens");
    assert_eq!(deposit_result.intent, "Deposit funds");

    // Wallet joins interpolated_intent (falling back to intent) with " and "
    let batch_summary = join_intents(&[&approve_result, &transfer_result, &deposit_result]);

    // Verify the joined summary contains all three intents
    let parts: Vec<&str> = batch_summary.split(" and ").collect();
    assert_eq!(parts.len(), 3, "expected 3 parts joined by ' and ', got: {batch_summary}");

    // approve has interpolatedIntent, so it should use that
    assert!(
        approve_result.interpolated_intent.is_some(),
        "approve descriptor should produce interpolated_intent"
    );

    // All intents present in the summary
    assert!(batch_summary.contains("Approve") || batch_summary.contains("approve"));
    assert!(batch_summary.contains("Transfer") || batch_summary.contains("transfer"));
    assert!(batch_summary.contains("Deposit") || batch_summary.contains("deposit"));
}

/// Full wallet flow: `format_calldata()` per inner call for display, then
/// `format_calldata_multi()` for the outer Safe `execTransaction` wrapper.
/// Verifies both layers produce valid output independently.
#[test]
fn wallet_batch_with_safe_wrapper() {
    let erc20_descriptor = load_descriptor("erc20-transfer.json");
    let safe_descriptor = load_descriptor("safe-exec-transaction.json");

    let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x1111111111111111111111111111111111111111";

    let mut tokens = StaticTokenSource::new();
    tokens.insert(
        1,
        usdc_addr,
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        },
    );

    let inner_calldata = build_erc20_transfer_calldata(recipient, 1_000_000); // 1 USDC

    // --- Step 1: Wallet formats the inner call individually for display ---
    let inner_display = format_calldata(
        &erc20_descriptor,
        1,
        usdc_addr,
        &inner_calldata,
        None,
        &tokens,
    )
    .unwrap();

    assert_eq!(inner_display.intent, "Transfer tokens");
    if let DisplayEntry::Item(ref item) = inner_display.entries[1] {
        assert_eq!(item.label, "Amount");
        assert_eq!(item.value, "1 USDC");
    } else {
        panic!("expected Item for inner Amount");
    }

    // --- Step 2: Wallet formats the outer Safe wrapper via format_calldata_multi ---
    let outer_calldata = build_exec_transaction_calldata(usdc_addr, 0, &inner_calldata, 0);

    let descriptors = vec![
        ResolvedDescriptor {
            descriptor: safe_descriptor,
            chain_id: 1,
            address: safe_addr.to_string(),
        },
        ResolvedDescriptor {
            descriptor: erc20_descriptor,
            chain_id: 1,
            address: usdc_addr.to_string(),
        },
    ];

    let safe_result = format_calldata_multi(
        &descriptors,
        1,
        safe_addr,
        &outer_calldata,
        None,
        None,
        &tokens,
    )
    .unwrap();

    // Verify outer Safe formatting
    assert_eq!(safe_result.intent, "Execute Safe transaction");

    // Verify nested inner call is rendered inside Safe wrapper
    match &safe_result.entries[2] {
        DisplayEntry::Nested {
            label,
            intent,
            entries,
            ..
        } => {
            assert_eq!(label, "Transaction");
            assert_eq!(intent, "Transfer tokens");
            assert!(
                entries.len() >= 2,
                "expected at least 2 inner entries, got {}",
                entries.len()
            );
            if let DisplayEntry::Item(ref item) = entries[1] {
                assert_eq!(item.label, "Amount");
                assert_eq!(item.value, "1 USDC");
            } else {
                panic!("expected Item for nested Amount");
            }
        }
        other => panic!("expected Nested for Transaction, got {:?}", other),
    }

    // Both layers produce valid, independent output
    assert!(!inner_display.intent.is_empty());
    assert!(!safe_result.intent.is_empty());
}
