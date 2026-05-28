use crate::compare::{CaseResult, Failure, FieldKind};

pub fn render_markdown(results: &[CaseResult]) -> String {
    let mut out = String::new();
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();
    out.push_str(&format!(
        "# cs-test report\n\n{passed}/{total} cases passed\n\n"
    ));

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
        Failure::IntentMismatch {
            path,
            expected,
            actual,
        } => {
            serde_json::json!({ "kind": "intent", "path": path, "expected": expected, "actual": actual })
        }
        Failure::InterpolatedIntentMismatch { expected, actual } => {
            serde_json::json!({ "kind": "interpolatedIntent", "expected": expected, "actual": actual })
        }
        Failure::OwnerMismatch {
            path,
            expected,
            actual,
        } => {
            serde_json::json!({ "kind": "owner", "path": path, "expected": expected, "actual": actual })
        }
        Failure::FieldMissing {
            path,
            label,
            expected_kind,
        } => {
            serde_json::json!({ "kind": "fieldMissing", "path": path, "label": label, "expectedKind": expected_kind.as_str() })
        }
        Failure::FieldExtra {
            path,
            label,
            actual_summary,
        } => {
            serde_json::json!({ "kind": "fieldExtra", "path": path, "label": label, "actual": actual_summary })
        }
        Failure::FieldValueMismatch {
            path,
            label,
            expected,
            actual,
        } => {
            serde_json::json!({ "kind": "fieldValue", "path": path, "label": label, "expected": expected, "actual": actual })
        }
        Failure::FieldKindMismatch {
            path,
            label,
            expected_kind,
            actual_kind,
        } => {
            serde_json::json!({ "kind": "fieldKind", "path": path, "label": label, "expectedKind": expected_kind.as_str(), "actualKind": actual_kind.as_str() })
        }
        Failure::AmbiguousLabel {
            path,
            label,
            actual_values,
        } => {
            serde_json::json!({ "kind": "ambiguousLabel", "path": path, "label": label, "actualValues": actual_values })
        }
    }
}

fn render_failure(f: &Failure) -> String {
    match f {
        Failure::IntentMismatch {
            path,
            expected,
            actual,
        } => {
            if path.is_empty() {
                format!("intent: expected {expected:?}, got {actual:?}")
            } else {
                format!(
                    "intent at {}: expected {expected:?}, got {actual:?}",
                    path.join(" > ")
                )
            }
        }
        Failure::InterpolatedIntentMismatch { expected, actual } => {
            format!("interpolated intent: expected {expected:?}, got {actual:?}")
        }
        Failure::OwnerMismatch {
            path,
            expected,
            actual,
        } => {
            if path.is_empty() {
                format!("owner: expected {expected:?}, got {actual:?}")
            } else {
                format!(
                    "owner at {}: expected {expected:?}, got {actual:?}",
                    path.join(" > ")
                )
            }
        }
        Failure::FieldMissing {
            path,
            label,
            expected_kind,
        } => {
            format!(
                "missing {} field {}",
                expected_kind.as_str(),
                labeled(path, label)
            )
        }
        Failure::FieldExtra {
            path,
            label,
            actual_summary,
        } => {
            format!(
                "unexpected field {} = {actual_summary:?}",
                labeled(path, label)
            )
        }
        Failure::FieldValueMismatch {
            path,
            label,
            expected,
            actual,
        } => {
            format!(
                "field {}: expected {expected:?}, got {actual:?}",
                labeled(path, label)
            )
        }
        Failure::FieldKindMismatch {
            path,
            label,
            expected_kind,
            actual_kind,
        } => {
            format!(
                "field kind {}: expected {}, got {}",
                labeled(path, label),
                expected_kind.as_str(),
                actual_kind.as_str()
            )
        }
        Failure::AmbiguousLabel {
            path,
            label,
            actual_values,
        } => {
            format!(
                "ambiguous field {}: rendered {} times with values {:?}",
                labeled(path, label),
                actual_values.len(),
                actual_values
            )
        }
    }
}

fn labeled(path: &[String], label: &str) -> String {
    if path.is_empty() {
        format!("{label:?}")
    } else {
        let joined = path.join(" > ");
        format!("\"{joined} > {label}\"")
    }
}

fn _kind_unused(_: FieldKind) {}
