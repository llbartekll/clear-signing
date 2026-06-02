use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel};
use clear_signing::outcome::FormatOutcome;

use crate::schema::{Expected, FieldEntry, FieldValue, NestedExpected};

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub description: String,
    pub passed: bool,
    pub failures: Vec<Failure>,
    /// Actual model rendered by the engine, when formatting succeeded.
    /// Absent only when the runner failed before producing a model (decode error,
    /// engine error, etc.) — in that case [`Self::error`] is set.
    pub model: Option<DisplayModel>,
    /// Set when the runner could not produce a model for this case at all.
    /// Distinguishes "ran and diverged" (`passed = false`, `error = None`) from
    /// "could not run" (`error = Some(_)`).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Scalar,
    Nested,
}

impl FieldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Scalar => "scalar",
            FieldKind::Nested => "nested",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Failure {
    IntentMismatch {
        path: Vec<String>,
        expected: String,
        actual: String,
    },
    InterpolatedIntentMismatch {
        expected: String,
        actual: Option<String>,
    },
    OwnerMismatch {
        path: Vec<String>,
        expected: Option<String>,
        actual: Option<String>,
    },
    FieldCountMismatch {
        path: Vec<String>,
        expected: usize,
        actual: usize,
    },
    FieldLabelMismatch {
        path: Vec<String>,
        index: usize,
        expected: String,
        actual: String,
    },
    FieldValueMismatch {
        path: Vec<String>,
        index: usize,
        label: String,
        expected: String,
        actual: String,
    },
    FieldKindMismatch {
        path: Vec<String>,
        index: usize,
        label: String,
        expected_kind: FieldKind,
        actual_kind: FieldKind,
    },
}

pub fn compare(description: &str, expected: &Expected, outcome: &FormatOutcome) -> CaseResult {
    let mut failures = Vec::new();
    let model = outcome.model();

    if expected.owner != model.owner {
        failures.push(Failure::OwnerMismatch {
            path: Vec::new(),
            expected: expected.owner.clone(),
            actual: model.owner.clone(),
        });
    }

    if let Some(exp_inter) = &expected.interpolated_intent {
        let actual = model.interpolated_intent.as_deref();
        if actual != Some(exp_inter.as_str()) {
            failures.push(Failure::InterpolatedIntentMismatch {
                expected: exp_inter.clone(),
                actual: model.interpolated_intent.clone(),
            });
        }
    }

    compare_level(
        &[],
        &expected.intent,
        &expected.fields,
        &model.intent,
        &model.entries,
        &mut failures,
    );

    CaseResult {
        description: description.to_string(),
        passed: failures.is_empty(),
        failures,
        model: Some(model.clone()),
        error: None,
    }
}

/// Build a CaseResult representing a runner-level error for a single case
/// (decode failure, engine error, etc.). Such a case has no rendered model.
pub fn case_error(description: &str, message: impl Into<String>) -> CaseResult {
    CaseResult {
        description: description.to_string(),
        passed: false,
        failures: Vec::new(),
        model: None,
        error: Some(message.into()),
    }
}

/// Concise human-readable description of the first divergence between expected
/// and rendered. Used in `results.json` on `fail` status. Returns `None` when
/// the case passed.
pub fn first_failure_message(result: &CaseResult) -> Option<String> {
    result.failures.first().map(failure_short_message)
}

fn failure_short_message(f: &Failure) -> String {
    match f {
        Failure::IntentMismatch {
            path,
            expected,
            actual,
        } => format!("intent{}: expected {expected:?}, got {actual:?}", at(path)),
        Failure::InterpolatedIntentMismatch { expected, actual } => format!(
            "interpolatedIntent: expected {expected:?}, got {}",
            opt_str_debug(actual.as_deref())
        ),
        Failure::OwnerMismatch {
            path,
            expected,
            actual,
        } => format!(
            "owner{}: expected {}, got {}",
            at(path),
            opt_str_debug(expected.as_deref()),
            opt_str_debug(actual.as_deref())
        ),
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

fn opt_str_debug(value: Option<&str>) -> String {
    match value {
        Some(v) => format!("{v:?}"),
        None => "<none>".to_string(),
    }
}

fn at(path: &[String]) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!(" at {}", path.join(" > "))
    }
}

fn compare_level(
    path: &[String],
    expected_intent: &str,
    expected_fields: &[FieldEntry],
    actual_intent: &str,
    actual_entries: &[DisplayEntry],
    failures: &mut Vec<Failure>,
) {
    if actual_intent != expected_intent {
        failures.push(Failure::IntentMismatch {
            path: path.to_vec(),
            expected: expected_intent.to_string(),
            actual: actual_intent.to_string(),
        });
    }

    let actual_pairs = flatten_actual(actual_entries);

    if expected_fields.len() != actual_pairs.len() {
        failures.push(Failure::FieldCountMismatch {
            path: path.to_vec(),
            expected: expected_fields.len(),
            actual: actual_pairs.len(),
        });
    }

    for (i, (exp, act)) in expected_fields.iter().zip(actual_pairs.iter()).enumerate() {
        if exp.label != act.label() {
            failures.push(Failure::FieldLabelMismatch {
                path: path.to_vec(),
                index: i,
                expected: exp.label.clone(),
                actual: act.label().to_string(),
            });
        }
        match (&exp.value, act) {
            (FieldValue::Value(expected_val), ActualField::Scalar { value, .. }) => {
                if expected_val.as_str() != *value {
                    failures.push(Failure::FieldValueMismatch {
                        path: path.to_vec(),
                        index: i,
                        label: exp.label.clone(),
                        expected: expected_val.clone(),
                        actual: (*value).to_string(),
                    });
                }
            }
            (
                FieldValue::Nested(ne),
                ActualField::Nested {
                    intent: actual_intent,
                    owner: actual_owner,
                    entries: actual_inner,
                    ..
                },
            ) => {
                let mut child_path = path.to_vec();
                child_path.push(format!("[{i}] {}", exp.label));
                let actual_owner_owned = actual_owner.map(str::to_string);
                if ne.owner != actual_owner_owned {
                    failures.push(Failure::OwnerMismatch {
                        path: child_path.clone(),
                        expected: ne.owner.clone(),
                        actual: actual_owner_owned,
                    });
                }
                compare_level(
                    &child_path,
                    &ne.intent,
                    &ne.fields,
                    actual_intent,
                    actual_inner,
                    failures,
                );
            }
            (FieldValue::Value(_), ActualField::Nested { .. }) => {
                failures.push(Failure::FieldKindMismatch {
                    path: path.to_vec(),
                    index: i,
                    label: exp.label.clone(),
                    expected_kind: FieldKind::Scalar,
                    actual_kind: FieldKind::Nested,
                });
            }
            (FieldValue::Nested(_), ActualField::Scalar { .. }) => {
                failures.push(Failure::FieldKindMismatch {
                    path: path.to_vec(),
                    index: i,
                    label: exp.label.clone(),
                    expected_kind: FieldKind::Nested,
                    actual_kind: FieldKind::Scalar,
                });
            }
        }
    }
}

enum ActualField<'a> {
    Scalar {
        label: &'a str,
        value: &'a str,
    },
    Nested {
        label: &'a str,
        intent: &'a str,
        owner: Option<&'a str>,
        entries: &'a [DisplayEntry],
    },
}

impl<'a> ActualField<'a> {
    fn label(&self) -> &'a str {
        match self {
            ActualField::Scalar { label, .. } => label,
            ActualField::Nested { label, .. } => label,
        }
    }
}

fn flatten_actual(entries: &[DisplayEntry]) -> Vec<ActualField<'_>> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            DisplayEntry::Item(DisplayItem { label, value }) => {
                out.push(ActualField::Scalar {
                    label: label.as_str(),
                    value: value.as_str(),
                });
            }
            DisplayEntry::Group { items, .. } => {
                for DisplayItem { label, value } in items {
                    out.push(ActualField::Scalar {
                        label: label.as_str(),
                        value: value.as_str(),
                    });
                }
            }
            DisplayEntry::Nested {
                label,
                intent,
                owner,
                entries,
            } => {
                out.push(ActualField::Nested {
                    label: label.as_str(),
                    intent: intent.as_str(),
                    owner: owner.as_deref(),
                    entries: entries.as_slice(),
                });
            }
        }
    }
    out
}

#[allow(dead_code)]
fn _nested_expected_type_check(_: &NestedExpected) {}

#[cfg(test)]
mod tests {
    use super::*;
    use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel};
    use clear_signing::outcome::FormatOutcome;

    fn outcome(model: DisplayModel) -> FormatOutcome {
        FormatOutcome::ClearSigned {
            model,
            diagnostics: vec![],
        }
    }

    fn model(
        intent: &str,
        interpolated: Option<&str>,
        owner: Option<&str>,
        entries: Vec<DisplayEntry>,
    ) -> DisplayModel {
        DisplayModel {
            intent: intent.to_string(),
            interpolated_intent: interpolated.map(str::to_string),
            entries,
            owner: owner.map(str::to_string),
            contract_name: None,
        }
    }

    fn item(label: &str, value: &str) -> DisplayEntry {
        DisplayEntry::Item(DisplayItem {
            label: label.to_string(),
            value: value.to_string(),
        })
    }

    fn nested(label: &str, intent: &str, entries: Vec<DisplayEntry>) -> DisplayEntry {
        nested_with_owner(label, intent, None, entries)
    }

    fn nested_with_owner(
        label: &str,
        intent: &str,
        owner: Option<&str>,
        entries: Vec<DisplayEntry>,
    ) -> DisplayEntry {
        DisplayEntry::Nested {
            label: label.to_string(),
            intent: intent.to_string(),
            owner: owner.map(str::to_string),
            entries,
        }
    }

    fn field(label: &str, value: &str) -> FieldEntry {
        FieldEntry {
            label: label.to_string(),
            value: FieldValue::Value(value.to_string()),
        }
    }

    fn nested_field(
        label: &str,
        intent: &str,
        owner: Option<&str>,
        fields: Vec<FieldEntry>,
    ) -> FieldEntry {
        FieldEntry {
            label: label.to_string(),
            value: FieldValue::Nested(NestedExpected {
                intent: intent.to_string(),
                owner: owner.map(str::to_string),
                fields,
            }),
        }
    }

    fn expected_with(intent: &str, fields: Vec<FieldEntry>) -> Expected {
        Expected {
            intent: intent.to_string(),
            interpolated_intent: None,
            owner: None,
            fields,
        }
    }

    #[test]
    fn duplicate_labels_compare_positionally_and_pass() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![
                item("Signer", "0xaaa"),
                item("Signer", "0xbbb"),
                item("Threshold", "2"),
            ],
        ));
        let exp = expected_with(
            "Outer",
            vec![
                field("Signer", "0xaaa"),
                field("Signer", "0xbbb"),
                field("Threshold", "2"),
            ],
        );
        let r = compare("t", &exp, &o);
        assert!(
            r.passed,
            "duplicate labels at distinct positions should compare equal: {:?}",
            r.failures
        );
    }

    #[test]
    fn duplicate_labels_swapped_fail_on_value() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![item("Signer", "0xbbb"), item("Signer", "0xaaa")],
        ));
        let exp = expected_with(
            "Outer",
            vec![field("Signer", "0xaaa"), field("Signer", "0xbbb")],
        );
        let r = compare("t", &exp, &o);
        let mismatches: Vec<_> = r
            .failures
            .iter()
            .filter(|f| matches!(f, Failure::FieldValueMismatch { .. }))
            .collect();
        assert_eq!(
            mismatches.len(),
            2,
            "expected both positions to mismatch, got {:?}",
            r.failures
        );
    }

    #[test]
    fn length_mismatch_reported() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![item("A", "1"), item("B", "2")],
        ));
        let exp = expected_with("Outer", vec![field("A", "1")]);
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::FieldCountMismatch {
                    expected: 1,
                    actual: 2,
                    ..
                }
            )
        });
        assert!(hit, "no FieldCountMismatch in {:?}", r.failures);
    }

    #[test]
    fn label_mismatch_at_position() {
        let o = outcome(model("Outer", None, None, vec![item("Foo", "1")]));
        let exp = expected_with("Outer", vec![field("Bar", "1")]);
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::FieldLabelMismatch { index: 0, expected, actual, .. } if expected == "Bar" && actual == "Foo"
        ));
        assert!(hit, "no FieldLabelMismatch in {:?}", r.failures);
    }

    #[test]
    fn nested_expected_matches_nested_actual() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![nested(
                "Transaction",
                "Inner",
                vec![item("Recipient", "0xabc")],
            )],
        ));
        let exp = expected_with(
            "Outer",
            vec![nested_field(
                "Transaction",
                "Inner",
                None,
                vec![field("Recipient", "0xabc")],
            )],
        );
        let r = compare("t", &exp, &o);
        assert!(r.passed, "expected pass, got failures: {:?}", r.failures);
    }

    #[test]
    fn nested_owner_matches_nested_actual() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![nested_with_owner(
                "Transaction",
                "Inner",
                Some("Inner DAO"),
                vec![],
            )],
        ));
        let exp = expected_with(
            "Outer",
            vec![nested_field(
                "Transaction",
                "Inner",
                Some("Inner DAO"),
                vec![],
            )],
        );
        let r = compare("t", &exp, &o);
        assert!(r.passed, "expected pass, got failures: {:?}", r.failures);
    }

    #[test]
    fn nested_owner_mismatch_reports_path() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![nested_with_owner(
                "Transaction",
                "Inner",
                Some("Actual DAO"),
                vec![],
            )],
        ));
        let exp = expected_with(
            "Outer",
            vec![nested_field(
                "Transaction",
                "Inner",
                Some("Expected DAO"),
                vec![],
            )],
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::OwnerMismatch { path, expected, actual }
                    if !path.is_empty()
                        && path[0].ends_with("Transaction")
                        && expected.as_deref() == Some("Expected DAO")
                        && actual.as_deref() == Some("Actual DAO")
            )
        });
        assert!(hit, "no nested-path OwnerMismatch in {:?}", r.failures);
    }

    #[test]
    fn nested_value_mismatch_reports_path() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![nested(
                "Transaction",
                "Inner",
                vec![item("Recipient", "0xWRONG")],
            )],
        ));
        let exp = expected_with(
            "Outer",
            vec![nested_field(
                "Transaction",
                "Inner",
                None,
                vec![field("Recipient", "0xabc")],
            )],
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::FieldValueMismatch { path, label, .. }
                    if !path.is_empty() && path[0].ends_with("Transaction") && label == "Recipient"
            )
        });
        assert!(hit, "no path-tagged FieldValueMismatch in {:?}", r.failures);
    }

    #[test]
    fn nested_intent_mismatch_reports_path() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![nested("Transaction", "ACTUAL", vec![])],
        ));
        let exp = expected_with(
            "Outer",
            vec![nested_field("Transaction", "EXPECTED", None, vec![])],
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::IntentMismatch { path, expected, actual }
                if !path.is_empty() && path[0].ends_with("Transaction") && expected == "EXPECTED" && actual == "ACTUAL"
        ));
        assert!(hit, "no nested-path IntentMismatch in {:?}", r.failures);
    }

    #[test]
    fn kind_mismatch_scalar_vs_nested() {
        let o = outcome(model("Outer", None, None, vec![item("X", "v")]));
        let exp = expected_with("Outer", vec![nested_field("X", "y", None, vec![])]);
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::FieldKindMismatch {
                    label,
                    expected_kind: FieldKind::Nested,
                    actual_kind: FieldKind::Scalar,
                    ..
                } if label == "X"
            )
        });
        assert!(hit, "no FieldKindMismatch in {:?}", r.failures);
    }

    #[test]
    fn kind_mismatch_nested_vs_scalar() {
        let o = outcome(model("Outer", None, None, vec![nested("X", "y", vec![])]));
        let exp = expected_with("Outer", vec![field("X", "v")]);
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::FieldKindMismatch {
                    label,
                    expected_kind: FieldKind::Scalar,
                    actual_kind: FieldKind::Nested,
                    ..
                } if label == "X"
            )
        });
        assert!(hit, "no FieldKindMismatch in {:?}", r.failures);
    }

    #[test]
    fn interpolated_intent_required_when_specified() {
        let o = outcome(model("Outer", Some("Got this"), None, vec![]));
        let exp = Expected {
            intent: "Outer".into(),
            interpolated_intent: Some("Want this".into()),
            owner: None,
            fields: vec![],
        };
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::InterpolatedIntentMismatch { expected, actual } if expected == "Want this" && actual.as_deref() == Some("Got this")
        ));
        assert!(hit, "no InterpolatedIntentMismatch in {:?}", r.failures);
    }

    #[test]
    fn interpolated_intent_skipped_when_omitted() {
        let o = outcome(model("Outer", Some("Whatever"), None, vec![]));
        let exp = expected_with("Outer", vec![]);
        let r = compare("t", &exp, &o);
        let has_inter = r
            .failures
            .iter()
            .any(|f| matches!(f, Failure::InterpolatedIntentMismatch { .. }));
        assert!(
            !has_inter,
            "should not check interpolated_intent when expected is None; got {:?}",
            r.failures
        );
    }

    #[test]
    fn nested_field_deserializes_with_owner() {
        let parsed: FieldEntry = serde_json::from_str(
            r#"{"label":"Inner","value":{"intent":"Inner","owner":"Inner DAO","fields":[{"label":"Amount","value":"1 USDC"}]}}"#,
        )
        .unwrap();

        assert_eq!(parsed.label, "Inner");
        match parsed.value {
            FieldValue::Nested(nested) => {
                assert_eq!(nested.intent, "Inner");
                assert_eq!(nested.owner.as_deref(), Some("Inner DAO"));
                assert_eq!(nested.fields.len(), 1);
                assert_eq!(nested.fields[0].label, "Amount");
                assert!(matches!(
                    &nested.fields[0].value,
                    FieldValue::Value(v) if v == "1 USDC"
                ));
            }
            other => panic!("expected nested field, got {other:?}"),
        }
    }
}
