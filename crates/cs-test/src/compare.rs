use std::collections::{BTreeSet, HashMap};

use clear_signing::engine::{DisplayEntry, DisplayItem};
use clear_signing::outcome::FormatOutcome;

use crate::schema::{Expected, FieldExpected, NestedExpected};

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub description: String,
    pub passed: bool,
    pub failures: Vec<Failure>,
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
    IntentMismatch { path: Vec<String>, expected: String, actual: String },
    InterpolatedIntentMismatch { expected: String, actual: Option<String> },
    OwnerMismatch { expected: Option<String>, actual: Option<String> },
    FieldMissing { path: Vec<String>, label: String, expected_kind: FieldKind },
    FieldExtra { path: Vec<String>, label: String, actual_summary: String },
    FieldValueMismatch { path: Vec<String>, label: String, expected: String, actual: String },
    FieldKindMismatch { path: Vec<String>, label: String, expected_kind: FieldKind, actual_kind: FieldKind },
    AmbiguousLabel { path: Vec<String>, label: String, actual_values: Vec<String> },
}

enum ActualField<'a> {
    Scalar(&'a str),
    Nested(&'a str, &'a [DisplayEntry]),
}

impl ActualField<'_> {
    fn kind(&self) -> FieldKind {
        match self {
            ActualField::Scalar(_) => FieldKind::Scalar,
            ActualField::Nested(_, _) => FieldKind::Nested,
        }
    }

    fn summary(&self) -> String {
        match self {
            ActualField::Scalar(v) => (*v).to_string(),
            ActualField::Nested(intent, _) => format!("<nested intent: {intent}>"),
        }
    }
}

pub fn compare(description: &str, expected: &Expected, outcome: &FormatOutcome) -> CaseResult {
    let mut failures = Vec::new();
    let model = outcome.model();

    if expected.owner != model.owner {
        failures.push(Failure::OwnerMismatch {
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

    CaseResult { description: description.to_string(), passed: failures.is_empty(), failures }
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
            DisplayEntry::Nested { label, intent, entries } => {
                actual_pairs.push((label.clone(), ActualField::Nested(intent.as_str(), entries.as_slice())));
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
            (FieldExpected::Nested(ne), ActualField::Nested(actual_intent, actual_inner)) => {
                let mut new_path = path.to_vec();
                new_path.push(label.clone());
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
        FormatOutcome::ClearSigned { model, diagnostics: vec![] }
    }

    fn model(intent: &str, interpolated: Option<&str>, owner: Option<&str>, entries: Vec<DisplayEntry>) -> DisplayModel {
        DisplayModel {
            intent: intent.to_string(),
            interpolated_intent: interpolated.map(str::to_string),
            entries,
            owner: owner.map(str::to_string),
            contract_name: None,
        }
    }

    fn item(label: &str, value: &str) -> DisplayEntry {
        DisplayEntry::Item(DisplayItem { label: label.to_string(), value: value.to_string() })
    }

    fn nested(label: &str, intent: &str, entries: Vec<DisplayEntry>) -> DisplayEntry {
        DisplayEntry::Nested { label: label.to_string(), intent: intent.to_string(), entries }
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
        let o = outcome(model("Outer", None, None, vec![
            nested("Transaction", "Inner", vec![item("Recipient", "0xabc")]),
        ]));
        let inner_fields = fields_with(&[("Recipient", FieldExpected::Value("0xabc".into()))]);
        let exp = expected_with("Outer", fields_with(&[(
            "Transaction",
            FieldExpected::Nested(NestedExpected { intent: "Inner".into(), fields: inner_fields }),
        )]));
        let r = compare("t", &exp, &o);
        assert!(r.passed, "expected pass, got failures: {:?}", r.failures);
    }

    #[test]
    fn nested_value_mismatch_reports_path() {
        let o = outcome(model("Outer", None, None, vec![
            nested("Transaction", "Inner", vec![item("Recipient", "0xWRONG")]),
        ]));
        let inner_fields = fields_with(&[("Recipient", FieldExpected::Value("0xabc".into()))]);
        let exp = expected_with("Outer", fields_with(&[(
            "Transaction",
            FieldExpected::Nested(NestedExpected { intent: "Inner".into(), fields: inner_fields }),
        )]));
        let r = compare("t", &exp, &o);
        assert!(!r.passed);
        let has_pathed_value = r.failures.iter().any(|f| matches!(
            f,
            Failure::FieldValueMismatch { path, label, .. } if path == &["Transaction".to_string()] && label == "Recipient"
        ));
        assert!(has_pathed_value, "no path-tagged FieldValueMismatch in {:?}", r.failures);
    }

    #[test]
    fn nested_intent_mismatch_reports_path() {
        let o = outcome(model("Outer", None, None, vec![
            nested("Transaction", "ACTUAL", vec![]),
        ]));
        let exp = expected_with("Outer", fields_with(&[(
            "Transaction",
            FieldExpected::Nested(NestedExpected { intent: "EXPECTED".into(), fields: IndexMap::new() }),
        )]));
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
        let exp = expected_with("Outer", fields_with(&[(
            "X",
            FieldExpected::Nested(NestedExpected { intent: "y".into(), fields: IndexMap::new() }),
        )]));
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
        let exp = expected_with("Outer", fields_with(&[("X", FieldExpected::Value("v".into()))]));
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
        let has_inter = r.failures.iter().any(|f| matches!(f, Failure::InterpolatedIntentMismatch { .. }));
        assert!(!has_inter, "should not check interpolated_intent when expected is None; got {:?}", r.failures);
    }

    #[test]
    fn duplicate_label_inside_nested_block_reports_correct_path() {
        let o = outcome(model("Outer", None, None, vec![
            nested("Transaction", "Inner", vec![
                item("Amount", "1"),
                item("Amount", "2"),
            ]),
        ]));
        let inner_fields = fields_with(&[("Amount", FieldExpected::Value("1".into()))]);
        let exp = expected_with("Outer", fields_with(&[(
            "Transaction",
            FieldExpected::Nested(NestedExpected { intent: "Inner".into(), fields: inner_fields }),
        )]));
        let r = compare("t", &exp, &o);
        let hit = r.failures.iter().any(|f| matches!(
            f,
            Failure::AmbiguousLabel { path, label, .. } if path == &["Transaction".to_string()] && label == "Amount"
        ));
        assert!(hit, "no path-tagged AmbiguousLabel in {:?}", r.failures);
        let value_compare_skipped = !r.failures.iter().any(|f| matches!(
            f,
            Failure::FieldValueMismatch { label, .. } if label == "Amount"
        ));
        assert!(value_compare_skipped, "should skip per-label compare when ambiguous");
    }
}
