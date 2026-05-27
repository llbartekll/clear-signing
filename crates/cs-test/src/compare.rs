use std::collections::{BTreeSet, HashMap};

use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel};
use clear_signing::outcome::FormatOutcome;

use crate::schema::{Expected, FieldExpected, NestedExpected};

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
    FieldMissing {
        path: Vec<String>,
        label: String,
        expected_kind: FieldKind,
    },
    FieldExtra {
        path: Vec<String>,
        label: String,
        actual_summary: String,
    },
    FieldValueMismatch {
        path: Vec<String>,
        label: String,
        expected: String,
        actual: String,
    },
    FieldKindMismatch {
        path: Vec<String>,
        label: String,
        expected_kind: FieldKind,
        actual_kind: FieldKind,
    },
    AmbiguousLabel {
        path: Vec<String>,
        label: String,
        actual_values: Vec<String>,
    },
}

enum ActualField<'a> {
    Scalar(&'a str),
    Nested {
        intent: &'a str,
        owner: Option<&'a str>,
        entries: &'a [DisplayEntry],
    },
}

impl ActualField<'_> {
    fn kind(&self) -> FieldKind {
        match self {
            ActualField::Scalar(_) => FieldKind::Scalar,
            ActualField::Nested { .. } => FieldKind::Nested,
        }
    }

    fn summary(&self) -> String {
        match self {
            ActualField::Scalar(v) => (*v).to_string(),
            ActualField::Nested { intent, .. } => format!("<nested intent: {intent}>"),
        }
    }
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
            format!(
                "interpolatedIntent: expected {expected:?}, got {}",
                opt_str_debug(actual.as_deref())
            )
        }
        Failure::OwnerMismatch {
            path,
            expected,
            actual,
        } => {
            if path.is_empty() {
                format!(
                    "owner: expected {}, got {}",
                    opt_str_debug(expected.as_deref()),
                    opt_str_debug(actual.as_deref())
                )
            } else {
                format!(
                    "owner at {}: expected {}, got {}",
                    path.join(" > "),
                    opt_str_debug(expected.as_deref()),
                    opt_str_debug(actual.as_deref())
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
                labeled_for_msg(path, label)
            )
        }
        Failure::FieldExtra {
            path,
            label,
            actual_summary,
        } => {
            format!(
                "unexpected field {} = {actual_summary:?}",
                labeled_for_msg(path, label)
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
                labeled_for_msg(path, label)
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
                labeled_for_msg(path, label),
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
                labeled_for_msg(path, label),
                actual_values.len(),
                actual_values
            )
        }
    }
}

fn opt_str_debug(value: Option<&str>) -> String {
    match value {
        Some(v) => format!("{v:?}"),
        None => "<none>".to_string(),
    }
}

fn labeled_for_msg(path: &[String], label: &str) -> String {
    if path.is_empty() {
        format!("{label:?}")
    } else {
        let joined = path.join(" > ");
        format!("\"{joined} > {label}\"")
    }
}

fn compare_level(
    path: &[String],
    expected_intent: &str,
    expected_fields: &indexmap::IndexMap<String, FieldExpected>,
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

    let mut actual_pairs: Vec<(String, ActualField)> = Vec::new();
    for e in actual_entries {
        match e {
            DisplayEntry::Item(DisplayItem { label, value }) => {
                actual_pairs.push((label.clone(), ActualField::Scalar(value.as_str())));
            }
            DisplayEntry::Group { items, .. } => {
                for DisplayItem { label, value } in items {
                    actual_pairs.push((label.clone(), ActualField::Scalar(value.as_str())));
                }
            }
            DisplayEntry::Nested {
                label,
                intent,
                owner,
                entries,
            } => {
                actual_pairs.push((
                    label.clone(),
                    ActualField::Nested {
                        intent: intent.as_str(),
                        owner: owner.as_deref(),
                        entries: entries.as_slice(),
                    },
                ));
            }
        }
    }

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (label, _) in &actual_pairs {
        *counts.entry(label.as_str()).or_insert(0) += 1;
    }

    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    for (label, _) in &actual_pairs {
        if counts.get(label.as_str()).copied().unwrap_or(0) > 1 && !ambiguous.contains(label) {
            let values: Vec<String> = actual_pairs
                .iter()
                .filter(|(l, _)| l == label)
                .map(|(_, f)| f.summary())
                .collect();
            failures.push(Failure::AmbiguousLabel {
                path: path.to_vec(),
                label: label.clone(),
                actual_values: values,
            });
            ambiguous.insert(label.clone());
        }
    }

    for (label, fe) in expected_fields {
        if ambiguous.contains(label) {
            continue;
        }
        let matches: Vec<&ActualField> = actual_pairs
            .iter()
            .filter(|(l, _)| l == label)
            .map(|(_, f)| f)
            .collect();
        if matches.is_empty() {
            failures.push(Failure::FieldMissing {
                path: path.to_vec(),
                label: label.clone(),
                expected_kind: expected_field_kind(fe),
            });
            continue;
        }
        let actual = matches[0];
        match (fe, actual) {
            (FieldExpected::Value(expected_val), ActualField::Scalar(actual_val)) => {
                if expected_val.as_str() != *actual_val {
                    failures.push(Failure::FieldValueMismatch {
                        path: path.to_vec(),
                        label: label.clone(),
                        expected: expected_val.clone(),
                        actual: (*actual_val).to_string(),
                    });
                }
            }
            (
                FieldExpected::Nested(ne),
                ActualField::Nested {
                    intent: actual_intent,
                    owner: actual_owner,
                    entries: actual_inner,
                },
            ) => {
                let mut new_path = path.to_vec();
                new_path.push(label.clone());
                let actual_owner = actual_owner.map(str::to_string);
                if ne.owner != actual_owner {
                    failures.push(Failure::OwnerMismatch {
                        path: new_path.clone(),
                        expected: ne.owner.clone(),
                        actual: actual_owner,
                    });
                }
                compare_level(
                    &new_path,
                    &ne.intent,
                    &ne.fields,
                    actual_intent,
                    actual_inner,
                    failures,
                );
            }
            _ => {
                failures.push(Failure::FieldKindMismatch {
                    path: path.to_vec(),
                    label: label.clone(),
                    expected_kind: expected_field_kind(fe),
                    actual_kind: actual.kind(),
                });
            }
        }
    }

    for (label, f) in &actual_pairs {
        if ambiguous.contains(label) {
            continue;
        }
        if !expected_fields.contains_key(label) {
            failures.push(Failure::FieldExtra {
                path: path.to_vec(),
                label: label.clone(),
                actual_summary: f.summary(),
            });
        }
    }
}

fn expected_field_kind(fe: &FieldExpected) -> FieldKind {
    match fe {
        FieldExpected::Value(_) => FieldKind::Scalar,
        FieldExpected::Nested(_) => FieldKind::Nested,
    }
}

#[allow(dead_code)]
fn _nested_expected_type_check(_: &NestedExpected) {}

#[cfg(test)]
mod tests {
    use super::*;
    use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel};
    use clear_signing::outcome::FormatOutcome;
    use indexmap::IndexMap;

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

    fn nested_expected(
        intent: &str,
        owner: Option<&str>,
        fields: IndexMap<String, FieldExpected>,
    ) -> FieldExpected {
        FieldExpected::Nested(NestedExpected {
            intent: intent.to_string(),
            owner: owner.map(str::to_string),
            fields,
        })
    }

    fn expected_with(intent: &str, fields: IndexMap<String, FieldExpected>) -> Expected {
        Expected {
            intent: intent.to_string(),
            interpolated_intent: None,
            owner: None,
            fields,
        }
    }

    fn fields_with(items: &[(&str, FieldExpected)]) -> IndexMap<String, FieldExpected> {
        let mut m = IndexMap::new();
        for (k, v) in items {
            m.insert((*k).to_string(), v.clone());
        }
        m
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
        let inner_fields = fields_with(&[("Recipient", FieldExpected::Value("0xabc".into()))]);
        let exp = expected_with(
            "Outer",
            fields_with(&[("Transaction", nested_expected("Inner", None, inner_fields))]),
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
            fields_with(&[(
                "Transaction",
                nested_expected("Inner", Some("Inner DAO"), IndexMap::new()),
            )]),
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
            fields_with(&[(
                "Transaction",
                nested_expected("Inner", Some("Expected DAO"), IndexMap::new()),
            )]),
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::OwnerMismatch { path, expected, actual }
                    if path == &["Transaction".to_string()]
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
        let inner_fields = fields_with(&[("Recipient", FieldExpected::Value("0xabc".into()))]);
        let exp = expected_with(
            "Outer",
            fields_with(&[("Transaction", nested_expected("Inner", None, inner_fields))]),
        );
        let r = compare("t", &exp, &o);
        assert!(!r.passed);
        let has_pathed_value = r.failures.iter().any(|f| matches!(
            f,
            Failure::FieldValueMismatch { path, label, .. } if path == &["Transaction".to_string()] && label == "Recipient"
        ));
        assert!(
            has_pathed_value,
            "no path-tagged FieldValueMismatch in {:?}",
            r.failures
        );
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
            fields_with(&[(
                "Transaction",
                nested_expected("EXPECTED", None, IndexMap::new()),
            )]),
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::IntentMismatch { path, expected, actual }
                if path == &["Transaction".to_string()] && expected == "EXPECTED" && actual == "ACTUAL"
        ));
        assert!(hit, "no nested-path IntentMismatch in {:?}", r.failures);
    }

    #[test]
    fn kind_mismatch_scalar_vs_nested() {
        let o = outcome(model("Outer", None, None, vec![item("X", "v")]));
        let exp = expected_with(
            "Outer",
            fields_with(&[("X", nested_expected("y", None, IndexMap::new()))]),
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::FieldKindMismatch { label, expected_kind: FieldKind::Nested, actual_kind: FieldKind::Scalar, .. } if label == "X"
        ));
        assert!(hit, "no FieldKindMismatch in {:?}", r.failures);
    }

    #[test]
    fn kind_mismatch_nested_vs_scalar() {
        let o = outcome(model("Outer", None, None, vec![nested("X", "y", vec![])]));
        let exp = expected_with(
            "Outer",
            fields_with(&[("X", FieldExpected::Value("v".into()))]),
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::FieldKindMismatch { label, expected_kind: FieldKind::Scalar, actual_kind: FieldKind::Nested, .. } if label == "X"
        ));
        assert!(hit, "no FieldKindMismatch in {:?}", r.failures);
    }

    #[test]
    fn interpolated_intent_required_when_specified() {
        let o = outcome(model("Outer", Some("Got this"), None, vec![]));
        let exp = Expected {
            intent: "Outer".into(),
            interpolated_intent: Some("Want this".into()),
            owner: None,
            fields: IndexMap::new(),
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
        let exp = expected_with("Outer", IndexMap::new());
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
    fn duplicate_label_inside_nested_block_reports_correct_path() {
        let o = outcome(model(
            "Outer",
            None,
            None,
            vec![nested(
                "Transaction",
                "Inner",
                vec![item("Amount", "1"), item("Amount", "2")],
            )],
        ));
        let inner_fields = fields_with(&[("Amount", FieldExpected::Value("1".into()))]);
        let exp = expected_with(
            "Outer",
            fields_with(&[("Transaction", nested_expected("Inner", None, inner_fields))]),
        );
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::AmbiguousLabel { path, label, .. } if path == &["Transaction".to_string()] && label == "Amount"
        ));
        assert!(hit, "no path-tagged AmbiguousLabel in {:?}", r.failures);
        let value_compare_skipped = !r.failures.iter().any(|f| {
            matches!(
                f,
                Failure::FieldValueMismatch { label, .. } if label == "Amount"
            )
        });
        assert!(
            value_compare_skipped,
            "should skip per-label compare when ambiguous"
        );
    }

    #[test]
    fn nested_expected_deserializes_with_owner() {
        let parsed: FieldExpected = serde_json::from_str(
            r#"{"intent":"Inner","owner":"Inner DAO","fields":{"Amount":"1 USDC"}}"#,
        )
        .unwrap();

        match parsed {
            FieldExpected::Nested(nested) => {
                assert_eq!(nested.intent, "Inner");
                assert_eq!(nested.owner.as_deref(), Some("Inner DAO"));
                assert!(matches!(
                    nested.fields.get("Amount"),
                    Some(FieldExpected::Value(value)) if value == "1 USDC"
                ));
            }
            other => panic!("expected nested field, got {other:?}"),
        }
    }

    #[test]
    fn nested_expected_deserializes_without_owner() {
        let parsed: FieldExpected =
            serde_json::from_str(r#"{"intent":"Inner","fields":{}}"#).unwrap();

        match parsed {
            FieldExpected::Nested(nested) => {
                assert_eq!(nested.intent, "Inner");
                assert_eq!(nested.owner, None);
                assert!(nested.fields.is_empty());
            }
            other => panic!("expected nested field, got {other:?}"),
        }
    }
}
