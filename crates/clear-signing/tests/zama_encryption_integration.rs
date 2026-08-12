//! Integration tests for the ERC-7730 `encryption` annotation on the Zama
//! ConfidentialWrapper `confidentialTransfer` call.
//!
//! Mirrors the sourcifyeth/clear-signing `feat/erc7730-encryption` behaviour:
//! an fhevm-encrypted `bytes32` amount handle is decrypted via the wallet's
//! `resolve_decrypted_value` callback, then rendered with the field's regular
//! `tokenAmount` format. When decryption is unavailable the field renders its
//! `fallbackLabel` with a `decryption_failed` diagnostic; a malformed annotation
//! is a descriptor bug and fails the format outright.

use std::collections::VecDeque;
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
    /// Answers handed out in order; the last one repeats, so a single-answer
    /// script models a wallet that always answers the same way.
    answers: Mutex<VecDeque<Option<String>>>,
    calls: Mutex<Vec<DecryptCall>>,
}

impl MockProvider {
    fn new(decrypted: Option<&str>) -> Self {
        Self::scripted([decrypted])
    }

    /// A wallet whose answers change per call — an interactive decryptor the
    /// user answers differently, or a session that expires mid-render.
    fn scripted<'a>(answers: impl IntoIterator<Item = Option<&'a str>>) -> Self {
        Self {
            answers: Mutex::new(
                answers
                    .into_iter()
                    .map(|a| a.map(str::to_string))
                    .collect::<VecDeque<_>>(),
            ),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn next_answer(&self) -> Option<String> {
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 {
            answers.pop_front().flatten()
        } else {
            answers.front().cloned().flatten()
        }
    }

    fn decrypt_calls(&self) -> usize {
        self.calls.lock().unwrap().len()
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
        let out = self.next_answer();
        Box::pin(async move { out })
    }
}

/// The first display item (the Amount field).
fn amount_item(entries: &[DisplayEntry]) -> &clear_signing::DisplayItem {
    match &entries[0] {
        DisplayEntry::Item(item) => {
            assert_eq!(item.label, "Amount");
            item
        }
        _ => panic!("expected an Item entry for Amount"),
    }
}

/// Value of the first display item (the Amount field).
fn amount_value(entries: &[DisplayEntry]) -> &str {
    &amount_item(entries).value
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
async fn rejects_a_malformed_encryption_annotation() {
    // A malformed annotation is a descriptor bug, not a decryption outcome: the
    // library cannot know what the field holds, so it refuses the transaction
    // rather than presenting it as understood. Rendering the fallback instead
    // would make a broken descriptor indistinguishable from a wallet that has no
    // keys — and would pass the registry fixtures silently.
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    for (enc, expected) in [
        // "euint64" is not a canonical Solidity type.
        (
            r#"{ "scheme": "fhevm", "plaintextType": "euint64", "fallbackLabel": "[Encrypted Amount]" }"#,
            "plaintextType",
        ),
        // Both `scheme` and `plaintextType` are required.
        (r#"{ "plaintextType": "uint64" }"#, "scheme"),
        (r#"{ "scheme": "fhevm" }"#, "scheme"),
    ] {
        let err = format_calldata(&descriptors(enc), &tx(&data), &provider)
            .await
            .expect_err("a malformed annotation must not render");
        let detail = format!("{err:?}");
        assert!(detail.contains(expected), "unexpected failure: {detail}");
    }

    // Never asks the wallet: the annotation is rejected before any round-trip.
    assert_eq!(provider.decrypt_calls(), 0);
}

#[tokio::test]
async fn rejects_a_malformed_encryption_annotation_on_a_hidden_field() {
    // The value is never read, so nothing would surface the bug at render time —
    // but the annotation is still part of the descriptor, and validating it costs
    // no wallet round-trip.
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    let field = r#"{ "path": "amount", "label": "Amount", "format": "number",
                     "visible": false,
                     "encryption": { "scheme": "fhevm", "plaintextType": "euint64" } }"#;
    format_calldata(&inline_descriptors(field, None), &tx(&data), &provider)
        .await
        .expect_err("a malformed annotation must not render");
    assert_eq!(provider.decrypt_calls(), 0);
}

// ---------------------------------------------------------------------------
// Declared `plaintextType` width — a decrypted `intN` must keep its sign
// ---------------------------------------------------------------------------

/// Descriptor whose `amount` field is inlined verbatim, with an optional
/// `interpolatedIntent`. Keeps these cases independent of the `$ref` fixture
/// above and lets each one pick its own format and visibility rule.
fn inline_descriptors(amount_field: &str, interpolated: Option<&str>) -> Vec<ResolvedDescriptor> {
    let interpolated = match interpolated {
        Some(template) => format!(r#""interpolatedIntent": "{template}","#),
        None => String::new(),
    };
    let json = format!(
        r#"{{
  "context": {{ "contract": {{ "deployments": [{{ "chainId": 1, "address": "{CONTRACT}" }}] }} }},
  "metadata": {{ "owner": "Zama", "contractName": "ConfidentialWrapper" }},
  "display": {{
    "definitions": {{}},
    "formats": {{
      "confidentialTransfer(address to, bytes32 amount)": {{
        "intent": "Confidential transfer",
        {interpolated}
        "fields": [
          {amount_field},
          {{ "path": "to", "label": "Receiver", "format": "raw" }}
        ]
      }}
    }}
  }}
}}"#
    );
    vec![ResolvedDescriptor {
        descriptor: Descriptor::from_json(&json).unwrap(),
        chain_id: 1,
        address: CONTRACT.to_string(),
    }]
}

/// An encrypted `amount` rendered as a plain number, so the decrypted integer is
/// read back exactly.
fn signed_amount_field(plaintext_type: &str) -> String {
    format!(
        r#"{{ "path": "amount", "label": "Amount", "format": "number",
              "encryption": {{ "scheme": "fhevm", "plaintextType": "{plaintext_type}",
                               "fallbackLabel": "[Encrypted Amount]" }} }}"#
    )
}

async fn rendered_amount(plaintext_type: &str, decrypted: &str) -> String {
    let provider = MockProvider::new(Some(decrypted));
    let data = calldata();
    let result = format_calldata(
        &inline_descriptors(&signed_amount_field(plaintext_type), None),
        &tx(&data),
        &provider,
    )
    .await
    .unwrap();
    amount_value(&result.entries).to_string()
}

#[tokio::test]
async fn int64_reads_a_minimal_positive_encoding_at_the_declared_width() {
    // 0xc8 is +200 under int64. Read as a one-byte two's complement value it
    // would be -56 — the declared width is what decides the sign.
    assert_eq!(rendered_amount("int64", "0xc8").await, "200");
}

#[tokio::test]
async fn int64_reads_a_full_width_positive_value() {
    assert_eq!(
        rendered_amount("int64", "0x7fffffffffffffff").await,
        i64::MAX.to_string()
    );
}

#[tokio::test]
async fn int64_reads_a_negative_value_at_the_declared_width() {
    // Two's complement at the declared width: int64 -56.
    assert_eq!(rendered_amount("int64", "0xffffffffffffffc8").await, "-56");
    assert_eq!(
        rendered_amount("int64", "0x8000000000000000").await,
        i64::MIN.to_string()
    );
}

#[tokio::test]
async fn uint64_reads_a_top_bit_set_plaintext_as_unsigned() {
    // max uint64. Its leading byte has the top bit set, which is what makes a
    // *signed* type depend on the declared width; `uint64` has no sign bit, so
    // the full range renders positive.
    assert_eq!(
        rendered_amount("uint64", "0xffffffffffffffff").await,
        u64::MAX.to_string()
    );
}

#[tokio::test]
async fn int64_reads_a_shortened_negative_as_the_positive_it_encodes() {
    // `0xff` is +255 at the declared width. The callback contract is that a
    // negative arrives at its full declared width, since the same bytes are a
    // different number at a different width.
    assert_eq!(rendered_amount("int64", "0xff").await, "255");
}

#[tokio::test]
async fn rejects_a_value_wider_than_the_declared_type() {
    // 9 significant bytes cannot be an int64. Padding a negative out to a wider
    // word is outside the callback contract too — only 0x00 carries no value, so
    // a 0xff-padded word is over-wide for signed and unsigned types alike, and
    // showing the fallback beats showing a wrong number.
    let sign_extended = format!("0x{}", "f".repeat(50) + "ffffffffffffffc8");
    for (plaintext_type, decrypted) in [
        ("int64", "0x0100000000000000c8"),
        ("int64", sign_extended.as_str()),
        ("uint64", sign_extended.as_str()),
    ] {
        assert_eq!(
            rendered_amount(plaintext_type, decrypted).await,
            "[Encrypted Amount]",
            "{plaintext_type} should reject {decrypted}"
        );
    }
}

// ---------------------------------------------------------------------------
// The wallet is asked exactly once per encrypted value
// ---------------------------------------------------------------------------

#[tokio::test]
async fn decrypts_once_for_both_the_entry_and_the_interpolated_intent() {
    // The Zama registry descriptor interpolates the encrypted amount, so the
    // field is rendered twice. This wallet would answer differently the second
    // time (an expired session, or a user declining the second prompt): the
    // entry and the intent must still agree, and the user must be asked once.
    let provider = MockProvider::scripted([Some("0x00000000000f4240"), None]);
    let data = calldata();
    let field = r#"{ "path": "amount", "label": "Amount", "format": "tokenAmount",
                     "params": { "tokenPath": "@.to" },
                     "encryption": { "scheme": "fhevm", "plaintextType": "uint64",
                                     "fallbackLabel": "[Encrypted Amount]" } }"#;
    let result = format_calldata(
        &inline_descriptors(field, Some("Send {amount}")),
        &tx(&data),
        &provider,
    )
    .await
    .unwrap();

    assert_eq!(amount_value(&result.entries), "1 cUSDC");
    assert_eq!(
        result.interpolated_intent.as_deref(),
        Some("Send 1 cUSDC"),
        "the intent must reuse the entry's plaintext, not decrypt again"
    );
    assert_eq!(provider.decrypt_calls(), 1);
}

#[tokio::test]
async fn never_asks_the_wallet_for_a_field_that_is_not_displayed() {
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    let field = r#"{ "path": "amount", "label": "Amount", "format": "number",
                     "visible": false,
                     "encryption": { "scheme": "fhevm", "plaintextType": "uint64" } }"#;
    let result = format_calldata(&inline_descriptors(field, None), &tx(&data), &provider)
        .await
        .unwrap();

    assert_eq!(result.entries.len(), 1, "only the Receiver is displayed");
    assert_eq!(
        provider.decrypt_calls(),
        0,
        "a hidden field must not trigger a decryption prompt"
    );
}

// ---------------------------------------------------------------------------
// Conditional visibility is evaluated on the plaintext, not the handle
// ---------------------------------------------------------------------------

/// `visible` conditions compare against the decoded value's JSON form, which for
/// an integer is its 32-byte word — the same shape a decrypted `uintN` takes.
fn uint_word_hex(value: u64) -> String {
    format!("0x{value:064x}")
}

fn conditional_amount_field(rule: &str) -> String {
    format!(
        r#"{{ "path": "amount", "label": "Amount", "format": "number",
              "visible": {rule},
              "encryption": {{ "scheme": "fhevm", "plaintextType": "uint64",
                               "fallbackLabel": "[Encrypted Amount]" }} }}"#
    )
}

#[tokio::test]
async fn must_match_is_checked_against_the_decrypted_plaintext() {
    // The handle never matches, so before decryption this aborted the whole
    // format; the plaintext does match, so the field is safely hidden.
    let rule = format!(r#"{{ "mustMatch": ["{}"] }}"#, uint_word_hex(1_000_000));
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    let result = format_calldata(
        &inline_descriptors(&conditional_amount_field(&rule), None),
        &tx(&data),
        &provider,
    )
    .await
    .unwrap();

    assert_eq!(result.entries.len(), 1, "matching field is hidden");
    assert_eq!(provider.decrypt_calls(), 1);
}

#[tokio::test]
async fn if_not_in_is_checked_against_the_decrypted_plaintext() {
    let rule = format!(r#"{{ "ifNotIn": ["{}"] }}"#, uint_word_hex(1_000_000));
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    let result = format_calldata(
        &inline_descriptors(&conditional_amount_field(&rule), None),
        &tx(&data),
        &provider,
    )
    .await
    .unwrap();

    assert_eq!(result.entries.len(), 1, "listed plaintext hides the field");
}

#[tokio::test]
async fn an_unverifiable_condition_never_hides_the_field() {
    // No decryptor: the plaintext is unknown, so the `ifNotIn` comparison cannot
    // be made. The field stays visible with its fallback label rather than
    // disappearing on a comparison against the ciphertext.
    let rule = format!(r#"{{ "ifNotIn": ["{}"] }}"#, uint_word_hex(1_000_000));
    let data = calldata();
    let result = format_calldata(
        &inline_descriptors(&conditional_amount_field(&rule), None),
        &tx(&data),
        &EmptyDataProvider,
    )
    .await
    .unwrap();

    assert_eq!(result.entries.len(), 2);
    assert_eq!(amount_value(&result.entries), "[Encrypted Amount]");
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}

#[tokio::test]
async fn an_unverifiable_must_match_is_not_passed_silently() {
    // `mustMatch` is a safety constraint: a value that cannot be read cannot be
    // verified, so the descriptor's promise fails rather than the field being
    // hidden or shown as verified.
    let rule = format!(r#"{{ "mustMatch": ["{}"] }}"#, uint_word_hex(1_000_000));
    let data = calldata();
    let err = format_calldata(
        &inline_descriptors(&conditional_amount_field(&rule), None),
        &tx(&data),
        &EmptyDataProvider,
    )
    .await
    .expect_err("an unverifiable mustMatch must not render as verified");
    assert!(
        format!("{err:?}").contains("mustMatch"),
        "unexpected failure: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// `rawEncryptedValue` — the handle is always reported, never rendered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reports_the_handle_alongside_the_decrypted_value() {
    // The spec RECOMMENDS wallets show the raw encrypted value next to the
    // field, so it is reported even when decryption succeeded and the plaintext
    // is on `value`.
    let provider = MockProvider::new(Some("0x00000000000f4240"));
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &provider)
        .await
        .unwrap();

    let amount = amount_item(&result.entries);
    assert_eq!(amount.value, "1 cUSDC");
    assert_eq!(amount.raw_encrypted_value.as_deref(), Some(HANDLE_HEX));

    // Only encrypted fields carry it.
    match &result.entries[1] {
        DisplayEntry::Item(receiver) => {
            assert_eq!(receiver.label, "Receiver");
            assert_eq!(receiver.raw_encrypted_value, None);
        }
        _ => panic!("expected an Item entry for Receiver"),
    }
}

#[tokio::test]
async fn reports_the_handle_when_decryption_is_unavailable() {
    // The point of the field: the value reads "[Encrypted Amount]", and the
    // wallet can still show what was actually signed beside it.
    let data = calldata();
    let result = format_calldata(&descriptors(FHEVM_UINT64), &tx(&data), &EmptyDataProvider)
        .await
        .unwrap();

    let amount = amount_item(&result.entries);
    assert_eq!(amount.value, "[Encrypted Amount]");
    assert_eq!(amount.raw_encrypted_value.as_deref(), Some(HANDLE_HEX));
    assert!(has_diagnostic(result.diagnostics(), "decryption_failed"));
}
