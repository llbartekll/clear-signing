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
        Failure::FieldCountMismatch {
            path,
            expected,
            actual,
        } => {
            serde_json::json!({ "kind": "fieldCount", "path": path, "expected": expected, "actual": actual })
        }
        Failure::FieldLabelMismatch {
            path,
            index,
            expected,
            actual,
        } => {
            serde_json::json!({ "kind": "fieldLabel", "path": path, "index": index, "expected": expected, "actual": actual })
        }
        Failure::FieldValueMismatch {
            path,
            index,
            label,
            expected,
            actual,
        } => {
            serde_json::json!({ "kind": "fieldValue", "path": path, "index": index, "label": label, "expected": expected, "actual": actual })
        }
        Failure::FieldKindMismatch {
            path,
            index,
            label,
            expected_kind,
            actual_kind,
        } => {
            serde_json::json!({
                "kind": "fieldKind",
                "path": path,
                "index": index,
                "label": label,
                "expectedKind": expected_kind.as_str(),
                "actualKind": actual_kind.as_str(),
            })
        }
    }
}

fn render_failure(f: &Failure) -> String {
    match f {
        Failure::IntentMismatch {
            path,
            expected,
            actual,
        } => format!("intent{}: expected {expected:?}, got {actual:?}", at(path)),
        Failure::InterpolatedIntentMismatch { expected, actual } => {
            format!("interpolated intent: expected {expected:?}, got {actual:?}")
        }
        Failure::OwnerMismatch {
            path,
            expected,
            actual,
        } => format!("owner{}: expected {expected:?}, got {actual:?}", at(path)),
        Failure::FieldCountMismatch {
            path,
            expected,
            actual,
        } => format!(
            "fields{}: expected {expected} entries, got {actual}",
            at(path)
        ),
        Failure::FieldLabelMismatch {
            path,
            index,
            expected,
            actual,
        } => format!(
            "field label{} at [{index}]: expected {expected:?}, got {actual:?}",
            at(path)
        ),
        Failure::FieldValueMismatch {
            path,
            index,
            label,
            expected,
            actual,
        } => format!(
            "field value{} at [{index}] {label:?}: expected {expected:?}, got {actual:?}",
            at(path)
        ),
        Failure::FieldKindMismatch {
            path,
            index,
            label,
            expected_kind,
            actual_kind,
        } => format!(
            "field kind{} at [{index}] {label:?}: expected {}, got {}",
            at(path),
            expected_kind.as_str(),
            actual_kind.as_str()
        ),
    }
}

fn at(path: &[String]) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!(" at {}", path.join(" > "))
    }
}

fn _kind_unused(_: FieldKind) {}
