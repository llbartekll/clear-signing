//! Regression coverage for bugs surfaced by the upstream registry's v2
//! test-migration PRs against `manuelwedler/clear-signing-erc7730-registry`
//! (#2 / #3 / #4).
//!
//! Each fixture is a descriptor plus its `.tests.json`, copied from the registry
//! branch's `testsv2/` directory (already in this runner's schema). The relative
//! `descriptor` path is adjusted for co-location. A fixture passes only when every
//! case renders exactly the registry's expected output, so these reproduce the CI
//! failures locally and were RED until each fix landed.
//!
//! Confirmed library bugs (active tests):
//!   #1 date suffix " UTC" should be "Z"        -> degate (eip712); also unit-tested
//!   #3 EIP-712 address not EIP-55 checksummed   -> degate
//!   #4 array-of-struct labels / <unresolved>    -> uniswap (eip712)
//!   #6 missing `interpolatedIntent`             -> lido
//!   #7 token amount rendered as raw integer     -> aave
//!   #8 `$.metadata.constants.*` not resolved    -> yieldxyz
//!
//! Harness capabilities exercised here (not engine bugs):
//!   - Nested calldata (kiln "Create and Stake"): the inner sub-call targets a
//!     separate contract; the runner indexes the vendored inner descriptor next to
//!     the outer so the engine resolves and renders it (owner + fields).
//!   - `@.from` (aave "Borrow" "Debtor"): the sender is supplied via the test
//!     case's `from`, as a wallet would — it is not recoverable from an unsigned
//!     rawTx.
//!   - #2 (zero amount "0.0" -> "0") is covered by unit + Safe integration
//!     assertions, not a vendored fixture.

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
    let results = run_file(&path, None, None)
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

/// Array-iteration `interpolatedIntent`: `"Withdraw {_amounts.[]}"` references a
/// `tokenAmount` field whose path is an array element (`#._amounts.[]`). A scalar
/// path resolve returns nothing for `.[]`, so the whole interpolatedIntent was
/// dropped; it must format each element and join with " and ".
#[tokio::test]
async fn lido_withdrawal_queue_matches_registry() {
    assert_matches_registry("lido/calldata-WithdrawalQueueERC721.tests.json").await;
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
/// native-currency amounts. The `Borrow` case also covers `@.from`: its "Debtor"
/// field reads the sender, supplied via the test case's `from` (the unsigned
/// rawTx carries no signature to recover it from).
#[tokio::test]
async fn aave_wrapped_token_gateway_matches_registry() {
    assert_matches_registry("aave/calldata-WrappedTokenGatewayV3.tests.json").await;
}

/// `senderAddress` given as a `$.metadata.constants.*` reference (paraswap
/// `addressAsNull` = `address(0)`). Every "Swap" case's `Beneficiary` field equals
/// that constant and must render "Sender"; previously the literal reference string
/// was compared against the address and never matched, so it rendered the raw zero
/// address. All 9 cases are flat swaps with token metadata from the test's provider.
#[tokio::test]
async fn paraswap_augustus_swapper_matches_registry() {
    assert_matches_registry("paraswap/calldata-AugustusSwapper-v6.2.tests.json").await;
}

/// Nested calldata. The "Create and Stake" call wraps a `createSplitterAndCall`
/// whose `data` targets a *different* contract (selector 0xfe37d829, "Stake any
/// amount per validator"). The runner indexes the vendored inner descriptor
/// (`calldata-kiln-batch-deposit-v2.json`) alongside the outer, so the engine
/// resolves and renders the inner call with its own owner.
#[tokio::test]
async fn kiln_fee_splitter_factory_matches_registry() {
    assert_matches_registry("kiln/calldata-kiln-fee-splitter-factory.tests.json").await;
}

/// `--registry` resolves nested-call descriptors from anywhere in an on-disk
/// registry tree, not just beside the test file. The fixture places the kiln
/// inner descriptor (`calldata-kiln-batch-deposit-v2.json`) under `ercs/`, a
/// different subdir than the test file's `kiln/`, so the sibling-only directory
/// scan cannot find it: the nested call resolves *with* `--registry` and not
/// without it.
#[tokio::test]
async fn registry_flag_resolves_nested_across_subdirs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/registry-tree");
    let test = root.join("kiln/calldata-kiln-fee-splitter-factory.tests.json");

    let with = run_file(&test, None, Some(&root))
        .await
        .expect("run with --registry");
    let failed: Vec<_> = with
        .iter()
        .filter(|r| !r.passed || r.error.is_some())
        .collect();
    assert!(
        failed.is_empty(),
        "expected all cases to pass with --registry; first divergence: {:?}",
        failed.first().and_then(|r| first_failure_message(r))
    );

    let without = run_file(&test, None, None)
        .await
        .expect("run without --registry");
    assert!(
        without.iter().any(|r| !r.passed),
        "expected the nested case to diverge without --registry (inner unresolved)"
    );
}
