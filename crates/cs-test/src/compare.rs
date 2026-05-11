use std::collections::{BTreeSet, HashMap};

use clear_signing::engine::{DisplayEntry, DisplayItem};
use clear_signing::outcome::FormatOutcome;

use crate::schema::Expected;

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub description: String,
    pub passed: bool,
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone)]
pub enum Failure {
    IntentMismatch { expected: String, actual: String },
    OwnerMismatch { expected: Option<String>, actual: Option<String> },
    FieldMissing { label: String, expected: String },
    FieldExtra { label: String, actual: String },
    FieldValueMismatch { label: String, expected: String, actual: String },
    AmbiguousLabel { label: String, actual_values: Vec<String> },
}

pub fn compare(description: &str, expected: &Expected, outcome: &FormatOutcome) -> CaseResult {
    let mut failures = Vec::new();
    let model = outcome.model();

    if model.intent != expected.intent {
        failures.push(Failure::IntentMismatch {
            expected: expected.intent.clone(),
            actual: model.intent.clone(),
        });
    }

    let actual_owner = model.owner.clone();
    if expected.owner != actual_owner {
        failures.push(Failure::OwnerMismatch {
            expected: expected.owner.clone(),
            actual: actual_owner,
        });
    }

    let actual_pairs = flatten_entries(&model.entries);

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (label, _) in &actual_pairs {
        *counts.entry(label.as_str()).or_insert(0) += 1;
    }

    let mut ambiguous_emitted: BTreeSet<String> = BTreeSet::new();
    for (label, _) in &actual_pairs {
        if counts.get(label.as_str()).copied().unwrap_or(0) > 1 && !ambiguous_emitted.contains(label) {
            let values: Vec<String> = actual_pairs
                .iter()
                .filter(|(l, _)| l == label)
                .map(|(_, v)| v.clone())
                .collect();
            failures.push(Failure::AmbiguousLabel { label: label.clone(), actual_values: values });
            ambiguous_emitted.insert(label.clone());
        }
    }

    let actual_unique: HashMap<&str, &str> = actual_pairs
        .iter()
        .filter(|(l, _)| counts.get(l.as_str()).copied().unwrap_or(0) == 1)
        .map(|(l, v)| (l.as_str(), v.as_str()))
        .collect();

    for (label, exp_val) in &expected.fields {
        if ambiguous_emitted.contains(label) {
            continue;
        }
        match actual_unique.get(label.as_str()) {
            Some(actual) if *actual == exp_val.as_str() => {}
            Some(actual) => failures.push(Failure::FieldValueMismatch {
                label: label.clone(),
                expected: exp_val.clone(),
                actual: actual.to_string(),
            }),
            None => failures.push(Failure::FieldMissing {
                label: label.clone(),
                expected: exp_val.clone(),
            }),
        }
    }

    for (label, val) in &actual_pairs {
        if ambiguous_emitted.contains(label) {
            continue;
        }
        if !expected.fields.contains_key(label) {
            failures.push(Failure::FieldExtra { label: label.clone(), actual: val.clone() });
        }
    }

    CaseResult { description: description.to_string(), passed: failures.is_empty(), failures }
}

fn flatten_entries(entries: &[DisplayEntry]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk(entries, &mut out);
    out
}

fn walk(entries: &[DisplayEntry], out: &mut Vec<(String, String)>) {
    for e in entries {
        match e {
            DisplayEntry::Item(DisplayItem { label, value }) => {
                out.push((label.clone(), value.clone()));
            }
            DisplayEntry::Group { items, .. } => {
                for DisplayItem { label, value } in items {
                    out.push((label.clone(), value.clone()));
                }
            }
            DisplayEntry::Nested { entries: inner, .. } => walk(inner, out),
        }
    }
}
