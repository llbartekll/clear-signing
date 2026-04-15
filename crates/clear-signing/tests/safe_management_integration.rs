//! Integration tests for Safe wallet management functions using real on-chain transactions.
//! Covers owner management, threshold changes, hash approval, and setup.
//! Note: execTransaction nesting is covered by safe_integration.rs.

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{CompositeDataProvider, StaticTokenSource, WellKnownTokenSource};
use clear_signing::types::context::{Deployment, DescriptorContext};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{
    format_calldata, DisplayEntry, DisplayModel, FallbackReason, FormatOutcome, TransactionContext,
};

fn load_descriptor(fixture: &str) -> Descriptor {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    Descriptor::from_json(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn wrap_rd(mut descriptor: Descriptor, chain_id: u64, address: &str) -> Vec<ResolvedDescriptor> {
    if let DescriptorContext::Contract(context) = &mut descriptor.context {
        context.contract.deployments = vec![Deployment {
            chain_id,
            address: address.to_string(),
        }];
    }

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

fn safe_provider() -> CompositeDataProvider {
    CompositeDataProvider::new(vec![
        Box::new(StaticTokenSource::new()),
        Box::new(WellKnownTokenSource::new()),
    ])
}

async fn run_safe_test(to: &str, calldata_hex: &str, from: &str) -> FormatOutcome {
    let descriptor = load_descriptor("common-Safe.json");
    let descriptors = wrap_rd(descriptor, 1, to);
    let calldata = decode_hex(calldata_hex);
    let provider = safe_provider();
    let tx = TransactionContext {
        chain_id: 1,
        to,
        calldata: &calldata,
        value: None,
        from: Some(from),
        implementation_address: None,
    };
    format_calldata(&descriptors, &tx, &provider).await.unwrap()
}

#[tokio::test]
async fn safe_add_owner_with_threshold() {
    let result = run_safe_test(
        "0xf9ea578233334bf9a6be61161f5c98820f7059fc",
        "0x0d582f13000000000000000000000000d0fce815af184cc758567a64ade6c49a1c2379f10000000000000000000000000000000000000000000000000000000000000002",
        "0x0f6a14d90c44b8731ea1f17b6e479a772171718c",
    ).await;
    assert!(result.is_clear_signed(), "expected clear-signed outcome");
    assert!(result.diagnostics().is_empty(), "unexpected diagnostics: {:?}", result.diagnostics());

    assert_eq!(result.intent, "Add signer");
    assert_eq!(result.owner.as_deref(), Some("Safe{Wallet}"));
    assert_eq!(
        get_entry_value(&result, "Signer"),
        "0xd0FCe815Af184CC758567a64Ade6c49A1c2379F1"
    );
    assert_eq!(get_entry_value(&result, "New threshold"), "2");
}

#[tokio::test]
async fn safe_remove_owner() {
    let result = run_safe_test(
        "0xf9ea578233334bf9a6be61161f5c98820f7059fc",
        "0xf8dc5dd9000000000000000000000000c6f80492d1a505b8da84a3c85d7a0ee1551e076e0000000000000000000000000f6a14d90c44b8731ea1f17b6e479a772171718c0000000000000000000000000000000000000000000000000000000000000001",
        "0x0f6a14d90c44b8731ea1f17b6e479a772171718c",
    ).await;
    assert!(result.is_clear_signed(), "expected clear-signed outcome");
    assert!(result.diagnostics().is_empty(), "unexpected diagnostics: {:?}", result.diagnostics());

    assert_eq!(result.intent, "Remove signer");
    assert_eq!(result.owner.as_deref(), Some("Safe{Wallet}"));
    assert_eq!(
        get_entry_value(&result, "Signer"),
        "0x0f6A14d90C44B8731ea1f17b6e479a772171718c"
    );
    assert_eq!(get_entry_value(&result, "New threshold"), "1");
}

#[tokio::test]
async fn safe_swap_owner() {
    let result = run_safe_test(
        "0xb45e9f74d0a35fe1aa0b78fea03877ef96ae8dd2",
        "0xe318b52b000000000000000000000000c30559a69c2654cdb7f1e04200037f026d9413130000000000000000000000002a3530417ef2ee48e0eb98cb4c097c87f853a66c000000000000000000000000b15115a15d5992a756d003ae74c0b832918fab75",
        "0xc841d6ddf66467af551b35218c0c2e22f9c14b48",
    ).await;
    assert!(result.is_clear_signed(), "expected clear-signed outcome");
    assert!(result.diagnostics().is_empty(), "unexpected diagnostics: {:?}", result.diagnostics());

    assert_eq!(result.intent, "Swap signer");
    assert_eq!(result.owner.as_deref(), Some("Safe{Wallet}"));
    assert_eq!(
        get_entry_value(&result, "Old signer"),
        "0x2A3530417Ef2eE48e0Eb98Cb4C097c87F853a66C"
    );
    assert_eq!(
        get_entry_value(&result, "New signer"),
        "0xb15115A15d5992A756D003AE74C0b832918fAb75"
    );
}

#[tokio::test]
async fn safe_change_threshold() {
    let result = run_safe_test(
        "0xf9ea578233334bf9a6be61161f5c98820f7059fc",
        "0x694e80c30000000000000000000000000000000000000000000000000000000000000002",
        "0xc6f80492d1a505b8da84a3c85d7a0ee1551e076e",
    )
    .await;
    assert!(result.is_clear_signed(), "expected clear-signed outcome");
    assert!(result.diagnostics().is_empty(), "unexpected diagnostics: {:?}", result.diagnostics());

    assert_eq!(result.intent, "Modify threshold");
    assert_eq!(result.owner.as_deref(), Some("Safe{Wallet}"));
    assert_eq!(get_entry_value(&result, "New threshold"), "2");
    assert_eq!(result.entries.len(), 1);
}

#[tokio::test]
async fn safe_approve_hash() {
    let result = run_safe_test(
        "0x849d52316331967b6ff1198e5e32a0eb168d039d",
        "0xd4d9bdcd8607b66e978423f17da142ffbf683741b7033cbe1b4334414fe505a72c22280c",
        "0xe9eb7da58f6b5ce5b0a6cfd778a2fa726203aad5",
    )
    .await;
    assert!(result.is_clear_signed(), "expected clear-signed outcome");
    assert!(result.diagnostics().is_empty(), "unexpected diagnostics: {:?}", result.diagnostics());

    assert_eq!(result.intent, "Approve Safe hash");
    assert_eq!(result.owner.as_deref(), Some("Safe{Wallet}"));
    assert_eq!(
        get_entry_value(&result, "Hash to approve"),
        "0x8607b66e978423f17da142ffbf683741b7033cbe1b4334414fe505a72c22280c"
    );
    assert_eq!(result.entries.len(), 1);
}

#[tokio::test]
async fn safe_setup() {
    let result = run_safe_test(
        "0x5052e00287b943538eb347bef5ce6639e63c9504",
        "0xb63e800d00000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000bd89a1ce4dde368ffab0ec35506eece0b1ffdc540000000000000000000000000000000000000000000000000000000000000140000000000000000000000000fd0732dc9e303f09fcef3a7388ad10a83459ec99000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005afe7a11e700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000005f5cd726d6210e80924c08d5cd114999f187c0990000000000000000000000000000000000000000000000000000000000000024fe51f64300000000000000000000000029fcb43b46531bca003ddc8fcb67ffe91900c76200000000000000000000000000000000000000000000000000000000",
        "0x5f5cd726d6210e80924c08d5cd114999f187c099",
    ).await;
    assert_eq!(
        result.fallback_reason(),
        Some(&FallbackReason::NestedCallNotClearSigned)
    );
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("No matching descriptor for inner call")),
        "expected nested fallback diagnostic, got {:?}",
        result.diagnostics()
    );

    assert_eq!(result.intent, "Setup Safe");
    assert_eq!(result.owner.as_deref(), Some("Safe{Wallet}"));
    assert_eq!(
        get_entry_value(&result, "Signer"),
        "0x5f5cd726d6210E80924C08D5Cd114999f187c099"
    );
    assert_eq!(get_entry_value(&result, "Threshold"), "1");
    assert_eq!(
        get_entry_value(&result, "Fallback handler"),
        "0xfd0732Dc9E303f09fCEf3a7388Ad10A83459Ec99"
    );
    assert_eq!(get_entry_value(&result, "Payment"), "0.0 ETH");

    // Verify nested module entry exists
    let has_nested = result
        .entries
        .iter()
        .any(|e| matches!(e, DisplayEntry::Nested { label, .. } if label == "Module"));
    assert!(has_nested, "setup should have a nested Module entry");
}
