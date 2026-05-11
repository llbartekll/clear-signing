use std::path::PathBuf;

use cs_test::compare::Failure;
use cs_test::runner::run_file;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

#[tokio::test]
async fn runner_pipeline_executes_and_reports_failures() {
    let path = fixture("smoke.tests.json");
    let results = run_file(&path, None).await.expect("run_file should succeed structurally");

    assert_eq!(results.len(), 1, "expected one case in smoke file");
    let r = &results[0];
    assert!(!r.passed, "smoke case has deliberately wrong expectations so it must fail");

    let kinds: Vec<&str> = r
        .failures
        .iter()
        .map(|f| match f {
            Failure::IntentMismatch { .. } => "intent",
            Failure::InterpolatedIntentMismatch { .. } => "interpolatedIntent",
            Failure::OwnerMismatch { .. } => "owner",
            Failure::FieldMissing { .. } => "missing",
            Failure::FieldExtra { .. } => "extra",
            Failure::FieldValueMismatch { .. } => "value",
            Failure::FieldKindMismatch { .. } => "kind",
            Failure::AmbiguousLabel { .. } => "ambiguous",
        })
        .collect();

    assert!(kinds.contains(&"intent"), "expected intent mismatch, got {kinds:?}");
    assert!(kinds.contains(&"owner"), "expected owner mismatch, got {kinds:?}");
    assert!(
        kinds.contains(&"extra"),
        "expected unrendered-by-test fields to surface as extra, got {kinds:?}"
    );
}

#[tokio::test]
async fn runner_reports_missing_descriptor() {
    let path = fixture("does-not-exist.tests.json");
    let err = run_file(&path, None).await.expect_err("missing file should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("read test file"), "unexpected error: {msg}");
}

#[tokio::test]
async fn runner_errors_when_case_filter_matches_nothing() {
    let path = fixture("smoke.tests.json");
    let err = run_file(&path, Some("does-not-exist-typo"))
        .await
        .expect_err("missing case filter must error, not silently pass");
    let msg = format!("{err:#}");
    assert!(msg.contains("--case"), "expected message to mention --case, got: {msg}");
}

#[tokio::test]
async fn runner_executes_eip712_pipeline() {
    let path = fixture("permit2.tests.json");
    let results = run_file(&path, None).await.expect("permit2 EIP-712 file should run");
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(!r.passed, "smoke file uses placeholder intent so case must fail");
    let has_intent_or_fields = r.failures.iter().any(|f| matches!(
        f,
        Failure::IntentMismatch { .. } | Failure::FieldExtra { .. }
    ));
    assert!(
        has_intent_or_fields,
        "expected EIP-712 pipeline to produce intent/field diffs, got {:?}",
        r.failures
    );
}
