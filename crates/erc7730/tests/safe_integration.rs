//! Integration tests for nested calldata (Safe execTransaction wrapping inner calls).

use erc7730::decoder::parse_signature;
use erc7730::provider::EmptyDataProvider;
use erc7730::resolver::ResolvedDescriptor;
use erc7730::token::{StaticTokenSource, TokenMeta};
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_calldata, DisplayEntry, TransactionContext};

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

/// Build ABI-encoded `execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)`
/// with the given inner calldata as the `data` parameter.
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

    // Param 0: to (address)
    calldata.extend_from_slice(&address_word(to));
    // Param 1: value (uint256)
    calldata.extend_from_slice(&uint_word(value));
    // Param 2: data (bytes) — offset pointer
    // Head section has 10 params × 32 bytes = 320 bytes offset
    calldata.extend_from_slice(&uint_word(320));
    // Param 3: operation (uint8)
    calldata.extend_from_slice(&uint_word(operation as u128));
    // Param 4: safeTxGas (uint256)
    calldata.extend_from_slice(&uint_word(0));
    // Param 5: baseGas (uint256)
    calldata.extend_from_slice(&uint_word(21000));
    // Param 6: gasPrice (uint256)
    calldata.extend_from_slice(&uint_word(0));
    // Param 7: gasToken (address)
    calldata.extend_from_slice(&[0u8; 32]);
    // Param 8: refundReceiver (address)
    calldata.extend_from_slice(&[0u8; 32]);
    // Param 9: signatures (bytes) — offset pointer
    let data_offset = 320 + 32 + pad32(inner_calldata.len());
    calldata.extend_from_slice(&uint_word(data_offset as u128));

    // Data section for param 2 (data bytes)
    calldata.extend_from_slice(&uint_word(inner_calldata.len() as u128)); // length
    calldata.extend_from_slice(inner_calldata);
    // Pad to 32-byte boundary
    let padding = pad32(inner_calldata.len()) - inner_calldata.len();
    calldata.extend_from_slice(&vec![0u8; padding]);

    // Data section for param 9 (signatures bytes) — empty
    calldata.extend_from_slice(&uint_word(0)); // length = 0

    calldata
}

fn pad32(len: usize) -> usize {
    len.div_ceil(32) * 32
}

/// Build inner ERC-20 transfer calldata: `transfer(address,uint256)`
fn build_erc20_transfer_calldata(to: &str, amount: u128) -> Vec<u8> {
    let sig = parse_signature("transfer(address,uint256)").unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(to));
    calldata.extend_from_slice(&uint_word(amount));
    calldata
}

#[tokio::test]
async fn safe_exec_transaction_wrapping_erc20_transfer() {
    let safe_descriptor = load_descriptor("common-Safe.json");
    let erc20_descriptor = load_descriptor("erc20-transfer.json");

    let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x1234567890123456789012345678901234567890";
    let amount = 1_000_000u128; // 1 USDC (6 decimals)

    let inner_calldata = build_erc20_transfer_calldata(recipient, amount);
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

    let tx = TransactionContext {
        chain_id: 1,
        to: safe_addr,
        calldata: &outer_calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    assert_eq!(result.intent, "sign multisig operation");

    // Check outer fields
    // Entry 0: Operation type = "Call"
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Operation type");
        assert_eq!(item.value, "Call");
    } else {
        panic!("expected Item for Operation type, got {:?}", result.entries[0]);
    }

    // Entry 1: From Safe (addressName for @.to)
    if let DisplayEntry::Item(ref item) = result.entries[1] {
        assert_eq!(item.label, "From Safe");
    } else {
        panic!("expected Item for From Safe, got {:?}", result.entries[1]);
    }

    // Entry 2: Execution signer (addressName for @.from) — not present when from is None

    // Entry 3 (or 2 if @.from absent): Transaction (Nested calldata)
    // Find the Nested entry by label since @.from may be absent
    let nested_idx = result
        .entries
        .iter()
        .position(|e| matches!(e, DisplayEntry::Nested { label, .. } if label == "Transaction"))
        .expect("expected Nested entry for Transaction");

    match &result.entries[nested_idx] {
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

            // Inner entry 0: To
            if let DisplayEntry::Item(ref item) = entries[0] {
                assert_eq!(item.label, "To");
            } else {
                panic!("expected Item for inner To");
            }

            // Inner entry 1: Amount — should have USDC formatting
            if let DisplayEntry::Item(ref item) = entries[1] {
                assert_eq!(item.label, "Amount");
                assert_eq!(item.value, "1 USDC");
            } else {
                panic!("expected Item for inner Amount");
            }
        }
        other => {
            panic!("expected Nested for Transaction, got {:?}", other);
        }
    }

    // Gas amount follows the Nested entry
    if let DisplayEntry::Item(ref item) = result.entries[nested_idx + 1] {
        assert_eq!(item.label, "Gas amount");
        assert_eq!(item.value, "21000");
    } else {
        panic!("expected Item for Gas amount");
    }

    // Gas price — now tokenAmount format with native currency
    if let DisplayEntry::Item(ref item) = result.entries[nested_idx + 2] {
        assert_eq!(item.label, "Gas price");
        assert_eq!(item.value, "0.0 ETH");
    } else {
        panic!("expected Item for Gas price");
    }
}

#[tokio::test]
async fn safe_exec_transaction_no_inner_descriptor() {
    let safe_descriptor = load_descriptor("common-Safe.json");
    let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
    let unknown_contract = "0x0000000000000000000000000000000000000042";

    // Some random inner calldata (unknown contract)
    let inner_calldata =
        hex::decode("12345678000000000000000000000000000000000000000000000000000000000000002a")
            .unwrap();
    let outer_calldata = build_exec_transaction_calldata(unknown_contract, 0, &inner_calldata, 0);

    let descriptors = vec![ResolvedDescriptor {
        descriptor: safe_descriptor,
        chain_id: 1,
        address: safe_addr.to_string(),
    }];

    let tx = TransactionContext {
        chain_id: 1,
        to: safe_addr,
        calldata: &outer_calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap();

    assert_eq!(result.intent, "sign multisig operation");

    // Transaction field should be a Nested with raw fallback — find by label
    let nested = result
        .entries
        .iter()
        .find(|e| matches!(e, DisplayEntry::Nested { label, .. } if label == "Transaction"))
        .expect("expected Nested entry for Transaction");

    match nested {
        DisplayEntry::Nested {
            label,
            intent,
            warnings,
            ..
        } => {
            assert_eq!(label, "Transaction");
            assert!(
                intent.contains("Unknown function"),
                "expected raw fallback intent, got: {intent}"
            );
            assert!(
                !warnings.is_empty(),
                "expected warnings for missing descriptor"
            );
        }
        other => {
            panic!("expected Nested for Transaction, got {:?}", other);
        }
    }
}

#[tokio::test]
async fn safe_exec_transaction_container_value_propagation() {
    let safe_descriptor = load_descriptor("common-Safe.json");
    let erc20_descriptor = load_descriptor("erc20-transfer.json");

    let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x1234567890123456789012345678901234567890";

    let inner_calldata = build_erc20_transfer_calldata(recipient, 500_000);
    // Set value = 1 ETH (1e18) in the outer call
    let outer_calldata =
        build_exec_transaction_calldata(usdc_addr, 1_000_000_000_000_000_000, &inner_calldata, 0);

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

    let tx = TransactionContext {
        chain_id: 1,
        to: safe_addr,
        calldata: &outer_calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &tokens).await.unwrap();

    // The inner call should still decode properly — find Nested by label
    let nested = result
        .entries
        .iter()
        .find(|e| matches!(e, DisplayEntry::Nested { label, .. } if label == "Transaction"))
        .expect("expected Nested entry for Transaction");

    match nested {
        DisplayEntry::Nested {
            intent, entries, ..
        } => {
            assert_eq!(intent, "Transfer tokens");
            // Inner Amount should still format correctly
            if let DisplayEntry::Item(ref item) = entries[1] {
                assert_eq!(item.label, "Amount");
                assert_eq!(item.value, "0.5 USDC");
            }
        }
        _ => panic!("expected Nested"),
    }
}

#[tokio::test]
async fn safe_exec_transaction_depth_limit() {
    let safe_descriptor = load_descriptor("common-Safe.json");
    let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";

    // Build 4-level nesting: Safe → Safe → Safe → Safe → ERC20
    // We need 4 levels because the depth check happens at entry to render_calldata_field,
    // so we need a calldata field at depth 3 (MAX_CALLDATA_DEPTH).
    let erc20_descriptor = load_descriptor("erc20-transfer.json");
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x1234567890123456789012345678901234567890";
    let erc20_calldata = build_erc20_transfer_calldata(recipient, 1_000_000);

    // Level 3: Safe wrapping ERC20
    let level3_calldata = build_exec_transaction_calldata(usdc_addr, 0, &erc20_calldata, 0);

    // Level 2: Safe wrapping Safe
    let level2_calldata = build_exec_transaction_calldata(safe_addr, 0, &level3_calldata, 0);

    // Level 1: Safe wrapping Safe
    let level1_calldata = build_exec_transaction_calldata(safe_addr, 0, &level2_calldata, 0);

    // Level 0: Safe wrapping everything
    let outer_calldata = build_exec_transaction_calldata(safe_addr, 0, &level1_calldata, 0);

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

    let tx = TransactionContext {
        chain_id: 1,
        to: safe_addr,
        calldata: &outer_calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap();

    // Verify the result doesn't panic and has nested structure
    assert_eq!(result.intent, "sign multisig operation");

    // Walk down to find the depth-limited entry
    fn find_depth_warning(entries: &[DisplayEntry], depth: usize) -> bool {
        for entry in entries {
            if let DisplayEntry::Nested {
                warnings, entries, ..
            } = entry
            {
                if warnings.iter().any(|w| w.contains("depth limit")) {
                    return true;
                }
                if depth < 10 && find_depth_warning(entries, depth + 1) {
                    return true;
                }
            }
        }
        false
    }

    assert!(
        find_depth_warning(&result.entries, 0),
        "expected depth limit warning somewhere in nested structure"
    );
}
