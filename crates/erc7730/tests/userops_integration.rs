//! Integration tests for ERC-4337 UserOps nested calldata via EIP-712.
//!
//! Tests the `#.` path prefix, EIP-712 → calldata nesting, and multi-level
//! UserOp → execute → ERC-20 transfer chains.

use erc7730::decoder::parse_signature;
use erc7730::eip712::TypedData;
use erc7730::provider::EmptyDataProvider;
use erc7730::resolver::ResolvedDescriptor;
use erc7730::token::{StaticTokenSource, TokenMeta};
use erc7730::types::descriptor::Descriptor;
use erc7730::DisplayEntry;

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

/// Build ERC-20 `transfer(address,uint256)` calldata.
fn build_erc20_transfer_calldata(to: &str, amount: u128) -> Vec<u8> {
    let sig = parse_signature("transfer(address,uint256)").unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(to));
    calldata.extend_from_slice(&uint_word(amount));
    calldata
}

/// Build smart account `execute(address,uint256,bytes)` calldata wrapping inner data.
fn build_execute_calldata(dest: &str, value: u128, inner_func: &[u8]) -> Vec<u8> {
    let sig = parse_signature("execute(address,uint256,bytes)").unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);

    // Param 0: dest (address)
    calldata.extend_from_slice(&address_word(dest));
    // Param 1: value (uint256)
    calldata.extend_from_slice(&uint_word(value));
    // Param 2: func (bytes) — offset pointer (3 params × 32 = 96)
    calldata.extend_from_slice(&uint_word(96));
    // Data section: length + data + padding
    calldata.extend_from_slice(&uint_word(inner_func.len() as u128));
    calldata.extend_from_slice(inner_func);
    let pad = inner_func.len().div_ceil(32) * 32 - inner_func.len();
    calldata.extend_from_slice(&vec![0u8; pad]);

    calldata
}

/// Build a PackedUserOperation EIP-712 typed data message with callData.
fn build_userop_typed_data(sender: &str, call_data: &[u8]) -> TypedData {
    let typed_data_json = serde_json::json!({
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"},
                {"name": "verifyingContract", "type": "address"}
            ],
            "PackedUserOperation": [
                {"name": "sender", "type": "address"},
                {"name": "nonce", "type": "uint256"},
                {"name": "callData", "type": "bytes"},
                {"name": "preVerificationGas", "type": "uint256"}
            ]
        },
        "primaryType": "PackedUserOperation",
        "domain": {
            "name": "Account",
            "version": "1",
            "chainId": 1,
            "verifyingContract": "0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789"
        },
        "message": {
            "sender": sender,
            "nonce": "1",
            "callData": format!("0x{}", hex::encode(call_data)),
            "preVerificationGas": "21000"
        }
    });

    serde_json::from_value(typed_data_json).unwrap()
}

#[tokio::test]
async fn userop_with_erc20_transfer_via_execute() {
    // Three-level nesting: UserOp (EIP-712) → execute (calldata) → transfer (calldata)
    let userop_descriptor = load_descriptor("userops-eip712.json");
    let account_descriptor = load_descriptor("smart-account-execute.json");
    let erc20_descriptor = load_descriptor("erc20-transfer.json");

    let sender = "0x1111111111111111111111111111111111111111";
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x2222222222222222222222222222222222222222";

    // Innermost: ERC-20 transfer
    let transfer_calldata = build_erc20_transfer_calldata(recipient, 5_000_000); // 5 USDC

    // Middle: smart account execute wrapping the transfer
    let execute_calldata = build_execute_calldata(usdc_addr, 0, &transfer_calldata);

    // Outermost: UserOp EIP-712 typed data
    let typed_data = build_userop_typed_data(sender, &execute_calldata);

    let descriptors = vec![
        ResolvedDescriptor {
            descriptor: account_descriptor,
            chain_id: 1,
            address: sender.to_string(),
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

    let mut all_descriptors = vec![ResolvedDescriptor {
        descriptor: userop_descriptor,
        chain_id: 1,
        address: "0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789".to_string(),
    }];
    all_descriptors.extend(descriptors);
    let result = erc7730::format_typed_data(&all_descriptors, &typed_data, &tokens)
        .await
        .unwrap();

    assert_eq!(result.intent, "Sign Packed User Operation");

    // Interpolated intent should include sender address
    assert!(
        result
            .interpolated_intent
            .as_ref()
            .unwrap()
            .contains(sender),
        "interpolated intent should contain sender: {:?}",
        result.interpolated_intent
    );

    // Entry 0: Sender Account
    if let DisplayEntry::Item(ref item) = result.entries[0] {
        assert_eq!(item.label, "Sender Account");
        // Should be the sender address (or resolved name)
        assert!(
            item.value
                .to_lowercase()
                .contains(&sender[2..].to_lowercase()),
            "sender value should contain sender address: {}",
            item.value
        );
    } else {
        panic!(
            "expected Item for Sender Account, got {:?}",
            result.entries[0]
        );
    }

    // Entry 1: Nonce — should be hidden (visible: "never"), so skip it
    // Entry 1 should be the Embedded Call (callData field)
    match &result.entries[1] {
        DisplayEntry::Nested {
            label,
            intent,
            entries,
            ..
        } => {
            assert_eq!(label, "Embedded Call");
            assert_eq!(intent, "Execute call");

            // The execute call should have: Destination, Value, Inner Call (nested)
            assert!(
                entries.len() >= 2,
                "expected at least 2 entries in execute, got {}",
                entries.len()
            );

            // Inner entry 0: Destination (checksummed address — account descriptor doesn't know USDC)
            if let DisplayEntry::Item(ref item) = entries[0] {
                assert_eq!(item.label, "Destination");
                assert!(
                    item.value
                        .to_lowercase()
                        .contains("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
                    "destination should contain USDC address: {}",
                    item.value
                );
            } else {
                panic!("expected Item for Destination");
            }

            // Inner entry 2: Inner Call (another Nested)
            let inner_call_idx = entries.len() - 1; // last entry should be the nested call
            match &entries[inner_call_idx] {
                DisplayEntry::Nested {
                    label,
                    intent,
                    entries: inner_entries,
                    ..
                } => {
                    assert_eq!(label, "Inner Call");
                    assert_eq!(intent, "Transfer tokens");

                    // Should have To and Amount
                    if let DisplayEntry::Item(ref item) = inner_entries[0] {
                        assert_eq!(item.label, "To");
                    }
                    if let DisplayEntry::Item(ref item) = inner_entries[1] {
                        assert_eq!(item.label, "Amount");
                        assert_eq!(item.value, "5 USDC");
                    }
                }
                other => {
                    panic!("expected Nested for Inner Call, got {:?}", other);
                }
            }
        }
        other => {
            panic!("expected Nested for Embedded Call, got {:?}", other);
        }
    }
}

#[tokio::test]
async fn userop_direct_erc20_transfer() {
    // Two-level nesting: UserOp (EIP-712) → transfer (calldata directly in callData)
    // Simulates an account whose callData IS the target call (no execute wrapper)
    let userop_descriptor = load_descriptor("userops-eip712.json");
    let erc20_descriptor = load_descriptor("erc20-transfer.json");

    // The sender IS the token contract (unusual but tests #. path resolution)
    let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x3333333333333333333333333333333333333333";

    let transfer_calldata = build_erc20_transfer_calldata(recipient, 2_000_000); // 2 USDC
    let typed_data = build_userop_typed_data(usdc_addr, &transfer_calldata);

    let descriptors = vec![ResolvedDescriptor {
        descriptor: erc20_descriptor,
        chain_id: 1,
        address: usdc_addr.to_string(),
    }];

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

    let mut all_descriptors = vec![ResolvedDescriptor {
        descriptor: userop_descriptor,
        chain_id: 1,
        address: "0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789".to_string(),
    }];
    all_descriptors.extend(descriptors);
    let result = erc7730::format_typed_data(&all_descriptors, &typed_data, &tokens)
        .await
        .unwrap();

    assert_eq!(result.intent, "Sign Packed User Operation");

    // Embedded Call should resolve to ERC-20 transfer via #.sender → USDC address
    match &result.entries[1] {
        DisplayEntry::Nested {
            label,
            intent,
            entries,
            ..
        } => {
            assert_eq!(label, "Embedded Call");
            assert_eq!(intent, "Transfer tokens");

            if let DisplayEntry::Item(ref item) = entries[1] {
                assert_eq!(item.label, "Amount");
                assert_eq!(item.value, "2 USDC");
            }
        }
        other => {
            panic!("expected Nested for Embedded Call, got {:?}", other);
        }
    }
}

#[tokio::test]
async fn userop_no_matching_inner_descriptor() {
    // UserOp with callData pointing to an unknown contract — graceful degradation
    let userop_descriptor = load_descriptor("userops-eip712.json");
    let unknown_sender = "0x9999999999999999999999999999999999999999";

    let random_calldata =
        hex::decode("12345678000000000000000000000000000000000000000000000000000000000000002a")
            .unwrap();
    let typed_data = build_userop_typed_data(unknown_sender, &random_calldata);

    // No inner descriptors provided
    let all_descriptors = vec![ResolvedDescriptor {
        descriptor: userop_descriptor,
        chain_id: 1,
        address: "0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789".to_string(),
    }];
    let result = erc7730::format_typed_data(&all_descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    assert_eq!(result.intent, "Sign Packed User Operation");

    // Embedded Call should degrade gracefully
    match &result.entries[1] {
        DisplayEntry::Nested {
            label,
            intent,
            warnings,
            ..
        } => {
            assert_eq!(label, "Embedded Call");
            assert!(
                intent.contains("Unknown function"),
                "expected raw fallback, got: {intent}"
            );
            assert!(
                !warnings.is_empty(),
                "expected warnings for missing descriptor"
            );
        }
        other => {
            panic!("expected Nested for Embedded Call, got {:?}", other);
        }
    }
}

#[tokio::test]
async fn userop_hash_prefix_resolves_from_message() {
    // Verify that `#.sender` resolves from the EIP-712 message field, not from calldata
    let userop_descriptor = load_descriptor("userops-eip712.json");
    let erc20_descriptor = load_descriptor("erc20-transfer.json");

    // Use the USDC address as sender — `#.sender` should resolve to this
    let sender = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let recipient = "0x4444444444444444444444444444444444444444";

    let transfer_calldata = build_erc20_transfer_calldata(recipient, 1_000_000);
    let typed_data = build_userop_typed_data(sender, &transfer_calldata);

    let descriptors = vec![ResolvedDescriptor {
        descriptor: erc20_descriptor,
        chain_id: 1,
        address: sender.to_string(), // sender == USDC contract
    }];

    let mut all_descriptors = vec![ResolvedDescriptor {
        descriptor: userop_descriptor,
        chain_id: 1,
        address: "0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789".to_string(),
    }];
    all_descriptors.extend(descriptors);
    let result = erc7730::format_typed_data(&all_descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    // The #.sender path should have resolved to the sender address from the message,
    // matched against the ERC-20 descriptor, and decoded the transfer calldata
    match &result.entries[1] {
        DisplayEntry::Nested {
            intent, entries, ..
        } => {
            assert_eq!(intent, "Transfer tokens");
            assert!(entries.len() >= 2);
            if let DisplayEntry::Item(ref item) = entries[0] {
                assert_eq!(item.label, "To");
            }
        }
        other => {
            panic!(
                "expected Nested with Transfer tokens intent, got {:?}",
                other
            );
        }
    }
}
