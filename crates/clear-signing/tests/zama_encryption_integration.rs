//! Integration tests for the ERC-7730 `encryption` annotation on the Zama
//! ConfidentialWrapper `confidentialTransfer` call.
//!
//! Mirrors the sourcifyeth/clear-signing `feat/erc7730-encryption` behaviour:
//! an fhevm-encrypted `bytes32` amount handle is decrypted via the wallet's
//! `resolve_decrypted_value` callback, then rendered with the field's regular
//! `tokenAmount` format. When decryption is unavailable the field renders its
//! `fallbackLabel`; a malformed annotation degrades the same way with a
//! distinct diagnostic.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{
    format_calldata, DataProvider, DisplayEntry, EmptyDataProvider, TokenMeta, TransactionContext,
};

// confidentialTransfer(address to, bytes32 amount) — same selector/handle as the
// sourcifyeth test fixture.
const CONTRACT: &str = "0xe978f22157048e5db8e5d07971376e86671672b2";
const HANDLE_HEX: &str = "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

/// Build the descriptor with a given `encryption` annotation JSON for the
/// `encryptedAmount` definition.
fn descriptor_json(encryption: &str) -> String {
    format!(
        r#"{{
  "context": {{ "contract": {{ "deployments": [{{ "chainId": 1, "address": "{CONTRACT}" }}] }} }},
  "metadata": {{ "owner": "Zama", "contractName": "ConfidentialWrapper" }},
  "display": {{
    "definitions": {{
      "encryptedAmount": {{
        "label": "Amount",
        "format": "tokenAmount",
        "params": {{ "tokenPath": "@.to" }},
        "encryption": {encryption}
      }}
    }},
    "formats": {{
      "confidentialTransfer(address to, bytes32 amount)": {{
        "intent": "Confidential transfer",
        "fields": [
          {{ "path": "amount", "$ref": "$.display.definitions.encryptedAmount" }},
          {{ "path": "to", "label": "Receiver", "format": "raw" }}
        ]
      }}
    }}
  }}
}}"#
    )
}

const FHEVM_UINT64: &str =
    r#"{ "scheme": "fhevm", "plaintextType": "uint64", "fallbackLabel": "[Encrypted Amount]" }"#;

fn descriptors(encryption: &str) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor: Descriptor::from_json(&descriptor_json(encryption)).unwrap(),
        chain_id: 1,
        address: CONTRACT.to_string(),
    }]
}

fn calldata() -> Vec<u8> {
    let mut d = vec![0x5b, 0xeb, 0xed, 0x7e]; // selector
    let mut to_word = [0u8; 32];
    to_word[12..]
        .copy_from_slice(&hex::decode("70997970c51812dc3a010c7d01b50e0d17dc79c8").unwrap());
    d.extend_from_slice(&to_word);
    d.extend_from_slice(&hex::decode(HANDLE_HEX.trim_start_matches("0x")).unwrap());
    d
}

fn tx(calldata: &[u8]) -> TransactionContext<'_> {
    TransactionContext {
        chain_id: 1,
        to: CONTRACT,
        calldata,
        value: None,
        from: None,
        implementation_address: None,
    }
}

/// A recorded `resolve_decrypted_value` call: (chain_id, encrypted_value, scheme, contract_address).
type DecryptCall = (u64, String, String, Option<String>);

/// A data provider that resolves the wrapper's token metadata and (optionally)
/// decrypts the handle, recording every decryption call it receives.
struct MockProvider {
    decrypted: Option<String>,
    calls: Mutex<Vec<DecryptCall>>,
}

impl MockProvider {
    fn new(decrypted: Option<&str>) -> Self {
        Self {
            decrypted: decrypted.map(str::to_string),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl DataProvider for MockProvider {
    fn resolve_token(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Option<TokenMeta>> + Send + '_>> {
        let hit = chain_id == 1 && address.eq_ignore_ascii_case(CONTRACT);
        Box::pin(async move {
            hit.then(|| TokenMeta {
                symbol: "cUSDC".to_string(),
                decimals: 6,
                name: "Confidential USDC".to_string(),
            })
        })
    }

    fn resolve_decrypted_value(
        &self,
        chain_id: u64,
        encrypted_value: &str,
        scheme: &str,
        contract_address: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        self.calls.lock().unwrap().push((
            chain_id,
            encrypted_value.to_string(),
            scheme.to_string(),
            contract_address.map(str::to_string),
        ));
        let out = self.decrypted.clone();
        Box::pin(async move { out })
    }
}

/// Value of the first display item (the Amount field).
fn amount_value(entries: &[DisplayEntry]) -> &str {
    match &entries[0] {
        DisplayEntry::Item(item) => {
            assert_eq!(item.label, "Amount");
            &item.value
        }
        _ => panic!("expected an Item entry for Amount"),
    }
}

fn has_diagnostic(diags: &[clear_signing::FormatDiagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code == code)
}

#[tokio::test]
async fn decrypts_handle_and_renders_token_amount() {
    // uint64 1000000, big-endian bytes, 0x-hex — the descriptor's plaintextType
    // tells the library to re-interpret it as a uint.
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(result.entries.len(), 2);
    // Decrypted 1000000 with 6 decimals → "1 cUSDC"
    assert_eq!(amount_value(&result.entries), "1 cUSDC");
    // Receiver is rendered raw (unencrypted)
    assert!(result.diagnostics().is_empty());

    // The wallet was handed the raw handle, the scheme, and the container `@.to`.
    let calls = provider.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 1);
    assert_eq!(calls[0].1, HANDLE_HEX);
    assert_eq!(calls[0].2, "fhevm");
    assert_eq!(
        calls[0].3.as_deref().map(|s| s.to_lowercase()),
        Some(CONTRACT.to_string())
    );
}

#[tokio::test]
async fn renders_fallback_label_when_no_provider() {
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &EmptyDataProvider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn renders_fallback_when_wallet_returns_none() {
    let provider = MockProvider::new(None);
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn reports_decryption_failed_on_invalid_hex() {
    // Odd-length hex — the `"0x" + n.toString(16)` bug on the wallet side.
    let provider = MockProvider::new(Some("0xf4240"));
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn reports_decryption_failed_when_plaintext_exceeds_width() {
    // 9 significant bytes for a declared uint64 (8 bytes) — over-wide, rejected.
    let provider = MockProvider::new(Some("0x0100000000000f4240"));
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn accepts_a_zero_padded_plaintext_whose_value_fits_the_type() {
    // A wallet returning a full 32-byte ABI word for a uint64: leading zeros
    // carry no value, so the value still fits and must be rendered.
    let padded = format!("0x{:064x}", 1_000_000u64);
    let provider = MockProvider::new(Some(&padded));
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "1 cUSDC");
    assert!(result.diagnostics().is_empty());
}

#[tokio::test]
async fn rejects_a_bytes_n_plaintext_wider_than_the_declared_size() {
    // For bytesN every byte is part of the value, so zero-padding does not help:
    // 32 bytes declared as bytes8 is too wide.
    let enc = r#"{ "scheme": "fhevm", "plaintextType": "bytes8", "fallbackLabel": "[Encrypted Amount]" }"#;
    let provider = MockProvider::new(Some(&format!("0x{:064x}", 1u64)));
    let data = calldata();
    let result = format_calldata(&descriptors(enc), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn generic_placeholder_when_no_fallback_label() {
    let provider = MockProvider::new(None);
    let enc = r#"{ "scheme": "fhevm", "plaintextType": "uint64" }"#;
    let data = calldata();
    let result = format_calldata(&descriptors(enc), &tx(&data), &provider)
        .await
        .unwrap();

    // Never the ciphertext — a generic placeholder when the descriptor declares
    // no fallbackLabel.
    assert_eq!(amount_value(&result.entries), "[Encrypted]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn invalid_descriptor_for_unsupported_plaintext_type() {
    // "euint64" is not a canonical Solidity type → descriptor bug.
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let enc = r#"{ "scheme": "fhevm", "plaintextType": "euint64", "fallbackLabel": "[Encrypted Amount]" }"#;
    let data = calldata();
    let result = format_calldata(&descriptors(enc), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(
        result.diagnostics(),
        "encryption_invalid_descriptor"
    ));
}
