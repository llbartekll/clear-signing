//! End-to-end fixture-driven tests for ERC-20 descriptor synthesis.
//!
//! Each fixture under `tests/fixtures/standard_token/` is a hand-built or
//! generator-produced snapshot — the calldata is real ABI-encoded ERC-20 input,
//! but seed fixtures use `0xseed-*` placeholder hashes. Run
//! `cargo run -p clear-signing --example fetch_erc20_fixtures --features github-registry`
//! after populating `CURATED` to replace them with snapshots of real
//! on-chain transactions fetched from Etherscan. The test runs the full
//! library pipeline (resolver + format_calldata) using a `StaticTokenSource`
//! populated from `token_meta` and asserts the rendered output matches the
//! committed `expected` block.

use std::path::PathBuf;

use clear_signing::resolver::StaticSource;
use clear_signing::token::StaticTokenSource;
use clear_signing::{
    format_calldata, resolve_descriptors_for_tx, DisplayEntry, TokenMeta, TransactionContext,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    chain_id: u64,
    to: String,
    #[serde(default)]
    from: Option<String>,
    calldata_hex: String,
    #[serde(default)]
    value_hex: Option<String>,
    token_meta: FixtureTokenMeta,
    expected: ExpectedOutput,
}

#[derive(Deserialize)]
struct FixtureTokenMeta {
    symbol: String,
    decimals: u8,
    name: String,
}

#[derive(Deserialize)]
struct ExpectedOutput {
    intent: String,
    interpolated_intent: String,
    fields: Vec<ExpectedField>,
}

#[derive(Deserialize)]
struct ExpectedField {
    label: String,
    value: String,
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/standard_token")
}

fn load_fixtures() -> Vec<(String, Fixture)> {
    let dir = fixture_dir();
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|err| {
        panic!("read fixtures dir {}: {err}", dir.display());
    });

    let mut fixtures = Vec::new();
    for entry in entries {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).expect("read fixture");
        let fixture: Fixture = serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
        fixtures.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            fixture,
        ));
    }
    fixtures.sort_by(|a, b| a.0.cmp(&b.0));
    fixtures
}

fn decode_hex(s: &str) -> Vec<u8> {
    let trimmed = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if !trimmed.len().is_multiple_of(2) {
        let padded = format!("0{trimmed}");
        return hex::decode(&padded).expect("hex decode");
    }
    hex::decode(trimmed).expect("hex decode")
}

#[tokio::test]
async fn fixture_outputs_match_committed_expectations() {
    let fixtures = load_fixtures();
    assert!(!fixtures.is_empty(), "at least one fixture must be present");

    for (name, fixture) in fixtures {
        let calldata = decode_hex(&fixture.calldata_hex);
        let value_bytes = fixture.value_hex.as_deref().map(decode_hex);
        let token_meta = TokenMeta {
            symbol: fixture.token_meta.symbol.clone(),
            decimals: fixture.token_meta.decimals,
            name: fixture.token_meta.name.clone(),
        };

        let mut tokens = StaticTokenSource::new();
        tokens.insert(fixture.chain_id, &fixture.to, token_meta);

        let source = StaticSource::new();
        let tx = TransactionContext {
            chain_id: fixture.chain_id,
            to: &fixture.to,
            calldata: &calldata,
            value: value_bytes.as_deref(),
            from: fixture.from.as_deref(),
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens))
            .await
            .unwrap_or_else(|err| panic!("[{name}] resolve: {err}"));
        assert_eq!(
            descriptors.len(),
            1,
            "[{name}] synth produces one descriptor"
        );

        let model = format_calldata(&descriptors, &tx, &tokens)
            .await
            .unwrap_or_else(|err| panic!("[{name}] format: {err}"));

        assert_eq!(
            model.intent, fixture.expected.intent,
            "[{name}] intent mismatch"
        );
        let interpolated = model
            .interpolated_intent
            .clone()
            .unwrap_or_else(|| panic!("[{name}] missing interpolated intent"));
        assert_eq!(
            interpolated, fixture.expected.interpolated_intent,
            "[{name}] interpolated intent mismatch"
        );

        let mut rendered_fields: Vec<(String, String)> = Vec::new();
        for entry in &model.entries {
            match entry {
                DisplayEntry::Item(item) => {
                    rendered_fields.push((item.label.clone(), item.value.clone()));
                }
                DisplayEntry::Group { items, .. } => {
                    for item in items {
                        rendered_fields.push((item.label.clone(), item.value.clone()));
                    }
                }
                DisplayEntry::Nested { .. } => {
                    panic!("[{name}] unexpected nested entry in top-level fixture")
                }
            }
        }

        assert_eq!(
            rendered_fields.len(),
            fixture.expected.fields.len(),
            "[{name}] field count mismatch (rendered={rendered_fields:?})"
        );
        for (rendered, expected) in rendered_fields.iter().zip(fixture.expected.fields.iter()) {
            assert_eq!(rendered.0, expected.label, "[{name}] field label");
            assert_eq!(
                rendered.1, expected.value,
                "[{name}] field value for label '{}'",
                expected.label
            );
        }
    }
}
