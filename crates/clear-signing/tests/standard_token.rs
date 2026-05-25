//! Integration tests for the standard ERC-20 descriptor synthesis path:
//! - the resolver short-circuits the registry source when both selector and token are known
//! - registry is still consulted when either signal is missing
//! - nested calls (Safe execTransaction) and EIP-712 wrappers also benefit

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use clear_signing::eip712::TypedData;
use clear_signing::resolver::{resolve_descriptors_for_typed_data, StaticSource};
use clear_signing::token::StaticTokenSource;
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{
    format_calldata, resolve_descriptors_for_tx, DescriptorSource, DisplayEntry,
    ResolvedDescriptor, TokenMeta, TransactionContext, TypedDescriptorLookup,
};

const USDC_ADDR: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
const RECIPIENT: &str = "0x1234567890123456789012345678901234567890";
const SPENDER: &str = "0xC6CDE7C39eb2f0F0095F41570af89eFC2C1Ea828";

// ---------------------------------------------------------------------------
// Recording source — counts resolve_calldata invocations
// ---------------------------------------------------------------------------

struct RecordingSource {
    inner: StaticSource,
    calldata_calls: Arc<AtomicUsize>,
}

impl RecordingSource {
    fn new() -> Self {
        Self {
            inner: StaticSource::new(),
            calldata_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn call_counter(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.calldata_calls)
    }

    fn add_calldata(&mut self, chain_id: u64, address: &str, descriptor: Descriptor) {
        self.inner.add_calldata(chain_id, address, descriptor);
    }

    fn add_typed(&mut self, chain_id: u64, address: &str, descriptor: Descriptor) {
        self.inner.add_typed(chain_id, address, descriptor);
    }
}

impl DescriptorSource for RecordingSource {
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<ResolvedDescriptor, clear_signing::error::ResolveError>>
                + Send
                + '_,
        >,
    > {
        self.calldata_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve_calldata(chain_id, address)
    }

    fn resolve_typed_candidates(
        &self,
        lookup: TypedDescriptorLookup,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Vec<ResolvedDescriptor>, clear_signing::error::ResolveError>>
                + Send
                + '_,
        >,
    > {
        self.inner.resolve_typed_candidates(lookup)
    }
}

// ---------------------------------------------------------------------------
// Calldata builders
// ---------------------------------------------------------------------------

fn address_word(hex_addr: &str) -> Vec<u8> {
    let hex = hex_addr
        .strip_prefix("0x")
        .or_else(|| hex_addr.strip_prefix("0X"))
        .unwrap_or(hex_addr);
    let bytes = hex::decode(hex).expect("hex addr");
    let mut word = vec![0u8; 12];
    word.extend_from_slice(&bytes);
    word
}

fn uint_word(val: u128) -> Vec<u8> {
    let mut word = vec![0u8; 16];
    word.extend_from_slice(&val.to_be_bytes());
    word
}

fn pad32(len: usize) -> usize {
    len.div_ceil(32) * 32
}

fn transfer_calldata(to: &str, amount: u128) -> Vec<u8> {
    let mut out = vec![0xa9, 0x05, 0x9c, 0xbb];
    out.extend_from_slice(&address_word(to));
    out.extend_from_slice(&uint_word(amount));
    out
}

/// Build transfer calldata with an explicit 32-byte amount — for testing uint256 max etc.
fn transfer_calldata_raw_amount(to: &str, amount_word: [u8; 32]) -> Vec<u8> {
    let mut out = vec![0xa9, 0x05, 0x9c, 0xbb];
    out.extend_from_slice(&address_word(to));
    out.extend_from_slice(&amount_word);
    out
}

fn approve_calldata(spender: &str, amount: u128) -> Vec<u8> {
    let mut out = vec![0x09, 0x5e, 0xa7, 0xb3];
    out.extend_from_slice(&address_word(spender));
    out.extend_from_slice(&uint_word(amount));
    out
}

/// Build approve calldata with an explicit 32-byte amount — for testing uint256 max etc.
fn approve_calldata_raw_amount(spender: &str, amount_word: [u8; 32]) -> Vec<u8> {
    let mut out = vec![0x09, 0x5e, 0xa7, 0xb3];
    out.extend_from_slice(&address_word(spender));
    out.extend_from_slice(&amount_word);
    out
}

fn transfer_from_calldata(from: &str, to: &str, amount: u128) -> Vec<u8> {
    let mut out = vec![0x23, 0xb8, 0x72, 0xdd];
    out.extend_from_slice(&address_word(from));
    out.extend_from_slice(&address_word(to));
    out.extend_from_slice(&uint_word(amount));
    out
}

fn exec_transaction_calldata(target: &str, inner: &[u8]) -> Vec<u8> {
    // execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)
    let selector = clear_signing::decoder::parse_signature(
        "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)",
    )
    .unwrap()
    .selector;

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&selector);
    calldata.extend_from_slice(&address_word(target));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(320));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&[0u8; 32]);
    calldata.extend_from_slice(&[0u8; 32]);
    let data_offset = 320 + 32 + pad32(inner.len());
    calldata.extend_from_slice(&uint_word(data_offset as u128));
    calldata.extend_from_slice(&uint_word(inner.len() as u128));
    calldata.extend_from_slice(inner);
    let padding = pad32(inner.len()) - inner.len();
    calldata.extend_from_slice(&vec![0u8; padding]);
    calldata.extend_from_slice(&uint_word(0));
    calldata
}

fn safe_descriptor() -> Descriptor {
    let path = format!(
        "{}/tests/fixtures/common-Safe.json",
        env!("CARGO_MANIFEST_DIR")
    );
    Descriptor::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn usdc_meta() -> TokenMeta {
    TokenMeta {
        symbol: "USDC".to_string(),
        decimals: 6,
        name: "USD Coin".to_string(),
    }
}

fn tokens_with_usdc() -> StaticTokenSource {
    let mut tokens = StaticTokenSource::new();
    tokens.insert(1, USDC_ADDR, usdc_meta());
    tokens
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn synth_transfer_renders_through_format_calldata() {
    let source = RecordingSource::new();
    let counter = source.call_counter();
    let tokens = tokens_with_usdc();

    let calldata = transfer_calldata(RECIPIENT, 2_500_000); // 2.5 USDC
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");
    assert_eq!(counter.load(Ordering::SeqCst), 0, "registry skipped");

    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");
    assert_eq!(model.intent, "Transfer tokens");
    let interpolated = model
        .interpolated_intent
        .clone()
        .expect("interpolated intent");
    assert!(
        interpolated.contains("2.5 USDC"),
        "expected '2.5 USDC' in '{interpolated}'"
    );
    assert!(
        interpolated.starts_with("Transfer "),
        "expected the synth interpolation template, got '{interpolated}'"
    );
}

#[tokio::test]
async fn synth_fires_when_proxy_caller_pre_sets_implementation_address() {
    // A direct caller pre-populates `implementation_address` for a proxy ERC-20
    // (e.g. they did their own EIP-1967 storage read upstream) and asks for
    // descriptors. The wallet's token list is keyed on the user-facing address
    // (tx.to), not the implementation. Synth must look up tokens against tx.to
    // while the synth descriptor's deployment uses the implementation so
    // format_calldata can match it.
    let impl_addr = "0x1111111111111111111111111111111111111111";
    let source = RecordingSource::new();
    let counter = source.call_counter();
    let tokens = tokens_with_usdc(); // keyed on USDC_ADDR (the proxy)

    let calldata = approve_calldata(SPENDER, 1_000_000);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: Some(impl_addr),
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");

    assert_eq!(descriptors.len(), 1, "synth fired");
    assert_eq!(counter.load(Ordering::SeqCst), 0, "registry not consulted");
    let synth = &descriptors[0];
    let deployment_addr = synth
        .descriptor
        .context
        .deployments()
        .first()
        .map(|d| d.address.clone())
        .expect("deployment present");
    assert_eq!(
        deployment_addr.to_lowercase(),
        impl_addr.to_lowercase(),
        "synth descriptor deploys at the implementation so format_calldata can match"
    );

    // Render through format_calldata to lock the integration end-to-end:
    // the descriptor matches against implementation_address and the amount
    // resolves via tokenPath: \"@.to\" → user-facing USDC_ADDR.
    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");
    assert_eq!(model.intent, "Approve token spending");
    let interpolated = model
        .interpolated_intent
        .clone()
        .expect("interpolated intent");
    assert!(
        interpolated.contains("1 USDC"),
        "expected '1 USDC' in '{interpolated}'"
    );
}

#[tokio::test]
async fn standard_selector_with_known_token_short_circuits_registry() {
    let source = RecordingSource::new();
    let counter = source.call_counter();
    let tokens = tokens_with_usdc();

    let calldata = approve_calldata(SPENDER, 1_000_000);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");

    assert_eq!(descriptors.len(), 1, "synth produces one descriptor");
    assert_eq!(counter.load(Ordering::SeqCst), 0, "registry not consulted");
    assert_eq!(
        descriptors[0]
            .descriptor
            .display
            .formats
            .keys()
            .next()
            .map(String::as_str),
        Some("approve(address spender,uint256 amount)")
    );
}

#[tokio::test]
async fn synth_wins_over_competing_registry_descriptor() {
    // Registry has an approve descriptor for the same token; synth should still win.
    let competing_json = r#"{
        "context": {
            "contract": {
                "deployments": [
                    { "chainId": 1, "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" }
                ]
            }
        },
        "metadata": {
            "owner": "Competing",
            "contractName": "Competing USDC",
            "enums": {}, "constants": {}, "maps": {}
        },
        "display": {
            "definitions": {},
            "formats": {
                "approve(address spender,uint256 amount)": {
                    "intent": "Competing approve",
                    "interpolatedIntent": "Competing approve {spender}",
                    "fields": [
                        { "path": "spender", "label": "Spender", "format": "addressName" },
                        { "path": "amount",  "label": "Amount",  "format": "raw" }
                    ]
                }
            }
        }
    }"#;
    let competing = Descriptor::from_json(competing_json).unwrap();
    let mut source = RecordingSource::new();
    source.add_calldata(1, USDC_ADDR, competing);
    let counter = source.call_counter();
    let tokens = tokens_with_usdc();

    let calldata = approve_calldata(SPENDER, 1_000_000);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");

    assert_eq!(descriptors.len(), 1);
    assert_eq!(counter.load(Ordering::SeqCst), 0, "registry skipped");
    let format = descriptors[0]
        .descriptor
        .display
        .formats
        .get("approve(address spender,uint256 amount)")
        .expect("synth format");
    let intent = format.intent.as_ref().expect("intent present");
    assert_eq!(
        intent.as_str(),
        Some("Approve token spending"),
        "synth intent, not competing"
    );
}

#[tokio::test]
async fn standard_selector_without_provider_falls_through_to_registry() {
    let mut source = RecordingSource::new();
    let mut registry = StaticSource::new();
    registry.add_calldata(
        1,
        USDC_ADDR,
        Descriptor::from_json(include_str!("fixtures/erc20-approve.json")).unwrap(),
    );
    // Mirror the registry contents into the recording source
    source.add_calldata(
        1,
        USDC_ADDR,
        Descriptor::from_json(include_str!("fixtures/erc20-approve.json")).unwrap(),
    );
    let counter = source.call_counter();

    let calldata = approve_calldata(SPENDER, 1_000_000);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, None)
        .await
        .expect("resolve");

    assert_eq!(descriptors.len(), 1);
    assert!(counter.load(Ordering::SeqCst) >= 1, "registry called");
}

#[tokio::test]
async fn non_standard_selector_with_known_token_uses_registry() {
    // WETH-style deposit() selector — not in the standard ERC-20 set.
    // Even though wallet knows the token, the synth must not fire.
    let mut source = RecordingSource::new();
    let weth_json = r#"{
        "context": {
            "contract": {
                "deployments": [
                    { "chainId": 1, "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" }
                ]
            }
        },
        "metadata": { "owner": "WETH", "enums": {}, "constants": {}, "maps": {} },
        "display": {
            "definitions": {},
            "formats": {
                "deposit()": {
                    "intent": "Wrap ETH",
                    "fields": []
                }
            }
        }
    }"#;
    source.add_calldata(1, USDC_ADDR, Descriptor::from_json(weth_json).unwrap());
    let counter = source.call_counter();
    let tokens = tokens_with_usdc();

    // deposit() selector
    let calldata = vec![0xd0, 0xe3, 0x0d, 0xb0];
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let _ = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");
    assert!(
        counter.load(Ordering::SeqCst) >= 1,
        "registry called for non-standard selector"
    );
}

#[tokio::test]
async fn eip712_wrapping_nested_calldata_synthesizes_inner_transfer() {
    // EIP-712 message wraps an inner ERC-20 transfer calldata via `format: calldata`.
    // Source has the outer EIP-712 descriptor but NOT the inner USDC descriptor.
    // Synth should fire for the inner transfer.
    let outer_address = "0x0000000000000000000000000000000000000abc";
    let outer_json = format!(
        r#"{{
            "context": {{
                "eip712": {{
                    "deployments": [{{ "chainId": 1, "address": "{outer_address}" }}],
                    "domain": {{ "name": "Nested Permit" }}
                }}
            }},
            "metadata": {{ "owner": "Outer Permit", "enums": {{}}, "constants": {{}}, "maps": {{}} }},
            "display": {{
                "definitions": {{}},
                "formats": {{
                    "Permit(address spender,Call call)Call(address to,bytes data)": {{
                        "intent": "Outer permit",
                        "fields": [{{
                            "path": "call.data",
                            "label": "Inner Call",
                            "format": "calldata",
                            "params": {{ "calleePath": "call.to" }}
                        }}]
                    }}
                }}
            }}
        }}"#
    );

    let mut source = RecordingSource::new();
    source.add_typed(
        1,
        outer_address,
        Descriptor::from_json(&outer_json).unwrap(),
    );
    let counter = source.call_counter();
    let tokens = tokens_with_usdc();

    let inner_calldata_hex = format!("0x{}", hex::encode(transfer_calldata(RECIPIENT, 1_500_000)));
    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "Permit": [
                { "name": "spender", "type": "address" },
                { "name": "call", "type": "Call" }
            ],
            "Call": [
                { "name": "to", "type": "address" },
                { "name": "data", "type": "bytes" }
            ],
            "EIP712Domain": [
                { "name": "name", "type": "string" },
                { "name": "chainId", "type": "uint256" },
                { "name": "verifyingContract", "type": "address" }
            ]
        },
        "domain": {
            "name": "Nested Permit",
            "chainId": "1",
            "verifyingContract": outer_address
        },
        "primaryType": "Permit",
        "message": {
            "spender": "0x00000000000000000000000000000000000000ff",
            "call": {
                "to": USDC_ADDR,
                "data": inner_calldata_hex,
            }
        }
    }))
    .expect("typed data");

    let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source, Some(&tokens))
        .await
        .expect("resolve");
    assert_eq!(descriptors.len(), 2, "outer + synthesized inner");
    assert_eq!(
        descriptors[1].address.to_lowercase(),
        USDC_ADDR.to_lowercase()
    );
    assert!(
        descriptors[1]
            .descriptor
            .display
            .formats
            .contains_key("transfer(address to,uint256 amount)"),
        "inner descriptor is the synthesized transfer"
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "registry not consulted for the inner call"
    );
}

#[tokio::test]
async fn nested_safe_exec_transaction_synthesizes_inner_approve() {
    let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
    let mut source = RecordingSource::new();
    source.add_calldata(1, safe_addr, safe_descriptor());
    // Note: no ERC-20 descriptor in the source — the inner approve must come from synth.
    let tokens = tokens_with_usdc();

    let inner = approve_calldata(SPENDER, 1_000_000);
    let outer = exec_transaction_calldata(USDC_ADDR, &inner);
    let tx = TransactionContext {
        chain_id: 1,
        to: safe_addr,
        calldata: &outer,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");

    assert_eq!(
        descriptors.len(),
        2,
        "outer Safe + inner synthesized ERC-20"
    );
    assert_eq!(
        descriptors[0].address.to_lowercase(),
        safe_addr.to_lowercase()
    );
    assert_eq!(
        descriptors[1].address.to_lowercase(),
        USDC_ADDR.to_lowercase()
    );
    let inner_intent = descriptors[1]
        .descriptor
        .display
        .formats
        .get("approve(address spender,uint256 amount)")
        .expect("inner approve format")
        .intent
        .as_ref()
        .expect("intent present");
    assert_eq!(
        inner_intent.as_str(),
        Some("Approve token spending"),
        "inner synth carries the standard intent"
    );

    // Render through format_calldata and assert the inner nested entry uses the plain intent.
    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");
    let nested = model
        .entries
        .iter()
        .find_map(|entry| match entry {
            DisplayEntry::Nested { intent, .. } => Some(intent.clone()),
            _ => None,
        })
        .expect("nested entry present");
    assert_eq!(
        nested, "Approve token spending",
        "nested rendering uses the plain intent, not the interpolated form"
    );
}

// ---------------------------------------------------------------------------
// Edge-case rendering: threshold/message + senderAddress
// ---------------------------------------------------------------------------

#[tokio::test]
async fn approve_with_uint256_max_renders_unlimited() {
    // Standard DeFi "infinite approval" — 1inch, Uniswap, Permit2 aggregators all
    // emit approve(spender, 2^256 - 1). The synth's threshold + message wires
    // the engine to render this as "Unlimited USDC" rather than the 70-digit
    // decimal expansion of the raw amount.
    let source = RecordingSource::new();
    let tokens = tokens_with_usdc();

    let calldata = approve_calldata_raw_amount(SPENDER, [0xff; 32]);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");
    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");
    let interpolated = model
        .interpolated_intent
        .clone()
        .expect("interpolated intent");
    assert!(
        interpolated.ends_with("to spend Unlimited USDC"),
        "expected 'to spend Unlimited USDC' suffix, got '{interpolated}'"
    );
    assert!(
        !interpolated.contains("115792089"),
        "the 70-digit decimal must not appear: '{interpolated}'"
    );
}

#[tokio::test]
async fn approve_with_uint256_max_minus_one_renders_full_amount() {
    // Locks the `>= max` semantics: exactly one less than uint256 max should
    // NOT trigger the "Unlimited" branch — it renders as the full decimal.
    // Prevents a future change from quietly loosening the bound.
    let source = RecordingSource::new();
    let tokens = tokens_with_usdc();

    let mut amount_word = [0xff_u8; 32];
    amount_word[31] = 0xfe;
    let calldata = approve_calldata_raw_amount(SPENDER, amount_word);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");
    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");
    let interpolated = model
        .interpolated_intent
        .clone()
        .expect("interpolated intent");
    assert!(
        !interpolated.contains("Unlimited"),
        "uint256_max - 1 must NOT trigger Unlimited: '{interpolated}'"
    );
    // The full decimal of (2^256 - 1) / 10^6 ends in ".639935" — check the
    // synth still rendered a numeric amount via tokenAmount.
    assert!(
        interpolated.contains("USDC"),
        "expected the token ticker in the rendered output: '{interpolated}'"
    );
}

#[tokio::test]
async fn transfer_with_uint256_max_renders_full_amount() {
    // Locks the Unlimited scoping: only `approve.amount` collapses at the cap.
    // A literal cap-valued transfer (rare, but possible — e.g. an attacker
    // crafting calldata to obscure UX) must render as the full decimal, not
    // "Unlimited", because `transfer` is not an allowance grant.
    let source = RecordingSource::new();
    let tokens = tokens_with_usdc();

    let calldata = transfer_calldata_raw_amount(SPENDER, [0xff; 32]);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");
    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");
    let interpolated = model
        .interpolated_intent
        .clone()
        .expect("interpolated intent");
    assert!(
        !interpolated.contains("Unlimited"),
        "transfer must NOT trigger Unlimited: '{interpolated}'"
    );
    assert!(
        interpolated.contains("USDC"),
        "expected the token ticker in the rendered output: '{interpolated}'"
    );
}

#[tokio::test]
async fn transfer_from_with_sender_as_from_renders_sender_label() {
    // senderAddress: "@.from" on every addressName field makes the engine
    // render "Sender" when the field address equals tx.from. Common in
    // delegated transferFrom flows where the caller controls the source.
    let source = RecordingSource::new();
    let tokens = tokens_with_usdc();

    // sender_addr is both `tx.from` AND the `from` argument of transferFrom.
    let sender_addr = "0xbf01daf454dce008d3e2bfd47d5e186f71477253";
    let calldata = transfer_from_calldata(sender_addr, RECIPIENT, 1_000_000);
    let tx = TransactionContext {
        chain_id: 1,
        to: USDC_ADDR,
        calldata: &calldata,
        value: None,
        from: Some(sender_addr),
        implementation_address: None,
    };

    let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
        .await
        .expect("resolve");
    let model = format_calldata(&descriptors, &tx, &tokens)
        .await
        .expect("format");

    let from_item = model
        .entries
        .iter()
        .find_map(|entry| match entry {
            DisplayEntry::Item(item) if item.label == "From" => Some(item.clone()),
            _ => None,
        })
        .expect("From field present");
    assert_eq!(
        from_item.value, "Sender",
        "from field matching tx.from should render as 'Sender'"
    );

    // Sanity-check the recipient field DOES still render as a checksummed
    // address (it doesn't match tx.from).
    let to_item = model
        .entries
        .iter()
        .find_map(|entry| match entry {
            DisplayEntry::Item(item) if item.label == "To" => Some(item.clone()),
            _ => None,
        })
        .expect("To field present");
    assert_ne!(
        to_item.value, "Sender",
        "to field NOT matching tx.from should render the address"
    );
}
