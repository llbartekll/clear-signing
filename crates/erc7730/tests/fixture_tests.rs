//! Snapshot-based fixture tests for ERC-7730 clear signing.
//!
//! Auto-discovers `tests/fixtures/*/tests.json` files and runs each test case
//! through `format_calldata_with_from()`, comparing against stored expected output.
//!
//! When `expected` is `null`, capture mode: populate and rewrite `tests.json`.

use erc7730::token::{CompositeTokenSource, StaticTokenSource, TokenMeta, WellKnownTokenSource};
use erc7730::types::descriptor::Descriptor;
use erc7730::{format_calldata_with_from, DisplayModel};
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize, serde::Serialize)]
struct FixtureSuite {
    descriptor: String,
    tokens: Vec<TokenEntry>,
    tests: Vec<TestCase>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct TokenEntry {
    chain_id: u64,
    address: String,
    symbol: String,
    decimals: u8,
    name: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct TestCase {
    name: String,
    tx_hash: String,
    chain_id: u64,
    to: String,
    calldata: String,
    value: String,
    from: String,
    expected: Option<serde_json::Value>,
}

fn discover_fixture_suites() -> Vec<PathBuf> {
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut suites = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&fixtures_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let tests_json = path.join("tests.json");
                if tests_json.exists() {
                    suites.push(tests_json);
                }
            }
        }
    }
    suites.sort();
    suites
}

fn load_descriptor(base_dir: &Path, relative: &str) -> Descriptor {
    let path = base_dir.join(relative);
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Descriptor::from_json(&json).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn build_token_source(tokens: &[TokenEntry]) -> CompositeTokenSource {
    let mut custom = StaticTokenSource::new();
    for t in tokens {
        custom.insert(
            t.chain_id,
            &t.address,
            TokenMeta {
                symbol: t.symbol.clone(),
                decimals: t.decimals,
                name: t.name.clone(),
            },
        );
    }
    CompositeTokenSource::new(vec![
        Box::new(custom),
        Box::new(WellKnownTokenSource::new()),
    ])
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
    // Pad to 32 bytes (big-endian)
    let raw = hex::decode(s).unwrap_or_else(|e| panic!("invalid value hex '{hex_str}': {e}"));
    if raw.is_empty() || raw.iter().all(|&b| b == 0) {
        return None;
    }
    let mut padded = vec![0u8; 32usize.saturating_sub(raw.len())];
    padded.extend_from_slice(&raw);
    Some(padded)
}

fn run_test_case(
    descriptor: &Descriptor,
    token_source: &dyn erc7730::TokenSource,
    tc: &TestCase,
) -> Result<DisplayModel, String> {
    let calldata = decode_hex(&tc.calldata);
    let val = value_bytes(&tc.value);

    format_calldata_with_from(
        descriptor,
        tc.chain_id,
        &tc.to,
        &calldata,
        val.as_deref(),
        Some(tc.from.as_str()),
        token_source,
    )
    .map_err(|e| format!("{e}"))
}

#[test]
fn fixture_snapshot_tests() {
    let suites = discover_fixture_suites();
    if suites.is_empty() {
        // No fixture directories yet — test is a no-op
        return;
    }

    let mut any_captured = false;
    let mut failures = Vec::new();

    for suite_path in &suites {
        let base_dir = suite_path.parent().unwrap();
        let suite_name = base_dir.file_name().unwrap().to_string_lossy().to_string();

        let json_str = std::fs::read_to_string(suite_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", suite_path.display()));
        let mut suite: FixtureSuite = serde_json::from_str(&json_str)
            .unwrap_or_else(|e| panic!("parse {}: {e}", suite_path.display()));

        let descriptor = load_descriptor(base_dir, &suite.descriptor);
        let token_source = build_token_source(&suite.tokens);

        for tc in &mut suite.tests {
            let result = run_test_case(&descriptor, &token_source, tc);

            match (&tc.expected, &result) {
                (None, Ok(model)) => {
                    // Capture mode: store actual output
                    tc.expected =
                        Some(serde_json::to_value(model).expect("serialize DisplayModel"));
                    any_captured = true;
                    eprintln!(
                        "[capture] {suite_name}/{}: captured expected output",
                        tc.name
                    );
                }
                (None, Err(e)) => {
                    failures.push(format!("{suite_name}/{}: capture failed: {e}", tc.name));
                }
                (Some(expected), Ok(model)) => {
                    let actual = serde_json::to_value(model).expect("serialize DisplayModel");
                    if &actual != expected {
                        let expected_pretty = serde_json::to_string_pretty(expected).unwrap();
                        let actual_pretty = serde_json::to_string_pretty(&actual).unwrap();
                        failures.push(format!(
                            "{suite_name}/{}: snapshot mismatch\n--- expected\n{expected_pretty}\n+++ actual\n{actual_pretty}",
                            tc.name
                        ));
                    }
                }
                (Some(_), Err(e)) => {
                    failures.push(format!(
                        "{suite_name}/{}: expected success but got error: {e}",
                        tc.name
                    ));
                }
            }
        }

        // Rewrite tests.json if any expected values were captured
        if any_captured {
            let updated = serde_json::to_string_pretty(&suite).expect("serialize suite");
            std::fs::write(suite_path, updated)
                .unwrap_or_else(|e| panic!("write {}: {e}", suite_path.display()));
            eprintln!(
                "[capture] Rewrote {} with captured snapshots",
                suite_path.display()
            );
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} fixture test(s) failed:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}
