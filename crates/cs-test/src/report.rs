use crate::compare::{CaseResult, Failure};

pub fn render_markdown(results: &[CaseResult]) -> String {
    let mut out = String::new();
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();
    out.push_str(&format!("# cs-test report\n\n{passed}/{total} cases passed\n\n"));

    for r in results {
        let icon = if r.passed { "✓" } else { "✗" };
        out.push_str(&format!("- {icon} {}\n", r.description));
        for f in &r.failures {
            out.push_str(&format!("    - {}\n", render_failure(f)));
        }
    }
    out
}

pub fn render_json(results: &[CaseResult]) -> String {
    let arr: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "description": r.description,
                "passed": r.passed,
                "failures": r.failures.iter().map(failure_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string())
}

fn failure_json(f: &Failure) -> serde_json::Value {
    match f {
        Failure::IntentMismatch { expected, actual } => {
            serde_json::json!({ "kind": "intent", "expected": expected, "actual": actual })
        }
        Failure::OwnerMismatch { expected, actual } => {
            serde_json::json!({ "kind": "owner", "expected": expected, "actual": actual })
        }
        Failure::FieldMissing { label, expected } => {
            serde_json::json!({ "kind": "fieldMissing", "label": label, "expected": expected })
        }
        Failure::FieldExtra { label, actual } => {
            serde_json::json!({ "kind": "fieldExtra", "label": label, "actual": actual })
        }
        Failure::FieldValueMismatch { label, expected, actual } => {
            serde_json::json!({ "kind": "fieldValue", "label": label, "expected": expected, "actual": actual })
        }
        Failure::AmbiguousLabel { label, actual_values } => {
            serde_json::json!({ "kind": "ambiguousLabel", "label": label, "actualValues": actual_values })
        }
    }
}

fn render_failure(f: &Failure) -> String {
    match f {
        Failure::IntentMismatch { expected, actual } => {
            format!("intent: expected {expected:?}, got {actual:?}")
        }
        Failure::OwnerMismatch { expected, actual } => {
            format!("owner: expected {expected:?}, got {actual:?}")
        }
        Failure::FieldMissing { label, expected } => {
            format!("missing field {label:?} (expected {expected:?})")
        }
        Failure::FieldExtra { label, actual } => {
            format!("unexpected field {label:?} = {actual:?}")
        }
        Failure::FieldValueMismatch { label, expected, actual } => {
            format!("field {label:?}: expected {expected:?}, got {actual:?}")
        }
        Failure::AmbiguousLabel { label, actual_values } => {
            format!(
                "ambiguous field {label:?}: rendered {} times with values {:?}",
                actual_values.len(),
                actual_values
            )
        }
    }
}
