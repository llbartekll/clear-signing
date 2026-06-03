//! Regression coverage for bugs surfaced by the upstream registry's v2
//! test-migration PRs against `manuelwedler/clear-signing-erc7730-registry`
//! (#2 / #3 / #4).
//!
//! Each fixture is a descriptor plus its `.tests.json`, copied from the registry
//! branch's `testsv2/` directory (already in this runner's schema). Deviations
//! from upstream: the relative `descriptor` path is adjusted for co-location, and
//! the aave `Borrow` case is dropped (see that test). A fixture passes only when
//! every case renders exactly the registry's expected output, so these reproduce
//! the CI failures locally and were RED until each fix landed.
//!
//! Confirmed library bugs (active tests):
//!   #1 date suffix " UTC" should be "Z"        -> degate (eip712); also unit-tested
//!   #3 EIP-712 address not EIP-55 checksummed   -> degate
//!   #4 array-of-struct labels / <unresolved>    -> uniswap (eip712)
//!   #6 missing `interpolatedIntent`             -> lido
//!   #7 token amount rendered as raw integer     -> aave
//!   #8 `$.metadata.constants.*` not resolved    -> yieldxyz
//!
//! Not pure library bugs (see `kiln`, below, and the diagnosis notes):
//!   - Nested-calldata cases (kiln fee-splitter, 1inch permitAndCall, safe/4337)
//!     fail because the inner sub-call targets a *separate* contract whose
//!     descriptor the cs-test runner never loads — `run_calldata` passes only the
//!     single outer descriptor. That is a harness-resolution gap, not a rendering
//!     bug, so #5 (nested owner) and the nested half of #7/#9 are tracked apart.
//!   - #2 (zero amount "0.0" -> "0") is fixed in `format_with_decimals`; it is
//!     covered by unit + Safe integration assertions, not a vendored fixture.

use std::path::PathBuf;

use cs_test::compare::first_failure_message;
use cs_test::runner::run_file;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/registry-regressions")
        .join(rel)
}

/// Run every case in a fixture and assert it matches the registry's expected
/// output. On divergence, panic with a CI-style one-line summary per failing
/// case so the red signal is actionable.
async fn assert_matches_registry(rel: &str) {
    let path = fixture(rel);
    let results = run_file(&path, None)
        .await
        .unwrap_or_else(|e| panic!("{rel}: harness could not run fixture: {e:#}"));

    let mut report = String::new();
    let mut failed = 0usize;
    for r in &results {
        if r.passed {
            continue;
        }
        failed += 1;
        let detail = if let Some(err) = &r.error {
            format!("runner error: {err}")
        } else {
            let first = first_failure_message(r).unwrap_or_else(|| "diverged".to_string());
            match r.failures.len().saturating_sub(1) {
                0 => first,
                extra => format!("{first} (+{extra} more divergence(s))"),
            }
        };
        report.push_str(&format!("\n  x {}\n      {detail}", r.description));
    }

    assert!(
        failed == 0,
        "{rel}: {failed}/{} case(s) diverge from registry expected:{report}\n",
        results.len()
    );
}

/// #6: `interpolatedIntent` is omitted entirely for the Lido descriptor set
/// (intent / owner / fields render correctly; only the interpolated string is
/// missing).
#[tokio::test]
async fn lido_wsteth_matches_registry() {
    assert_matches_registry("lido/calldata-wstETH.tests.json").await;
}

/// #8: token-amount ticker given as `$.metadata.constants.*` is emitted
/// literally instead of being resolved (e.g. `113.82$.metadata.constants...`).
#[tokio::test]
async fn yieldxyz_pol_validator_matches_registry() {
    assert_matches_registry("yieldxyz/calldata-yieldxyz-pol-validator.tests.json").await;
}

/// #3 + #1: EIP-712 address fields echo the message's raw casing instead of
/// EIP-55 checksumming, and timestamps render with a " UTC" suffix instead of
/// the RFC-3339 "Z".
#[tokio::test]
async fn degate_eip712_matches_registry() {
    assert_matches_registry("degate/eip712-degate.tests.json").await;
}

/// #4: EIP-712 array-of-struct (UniswapX V2 Dutch order outputs) renders the
/// wrong field labels/order and yields `<unresolved>` addresses.
#[tokio::test]
async fn uniswap_v2_dutch_order_matches_registry() {
    assert_matches_registry("uniswap/eip712-uniswap-V2DutchOrder.tests.json").await;
}

/// #7: top-level `amount`-format fields rendered as raw integers instead of
/// native-currency amounts. The registry's `Borrow` case is omitted here: its
/// "Debtor" field reads `@.from`, which the runner does not provide (sender
/// recovery is a harness gap, like nested-call resolution).
#[tokio::test]
async fn aave_wrapped_token_gateway_matches_registry() {
    assert_matches_registry("aave/calldata-WrappedTokenGatewayV3.tests.json").await;
}

/// Harness gap, NOT a library rendering bug. The "Create and Stake" call wraps a
/// nested `createSplitterAndCall` whose `data` targets a *different* contract
/// (selector 0xfe37d829, "Stake any amount per validator"). The cs-test runner
/// loads only the single outer descriptor, so the engine cannot decode the inner
/// call and emits raw params + no nested owner. Resolving this needs the runner
/// to provide inner descriptors (as the registry's reference runner does), not a
/// change to the engine. Kept as documented evidence; ignored until the runner
/// learns nested-descriptor resolution.
#[tokio::test]
#[ignore = "cs-test runner does not resolve nested-call descriptors yet (harness gap, not a library bug)"]
async fn kiln_fee_splitter_factory_matches_registry() {
    assert_matches_registry("kiln/calldata-kiln-fee-splitter-factory.tests.json").await;
}
