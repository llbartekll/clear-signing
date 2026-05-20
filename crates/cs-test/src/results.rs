//! Registry-compatible `results.json` emission.
//!
//! Output format is defined by the `manuelwedler/clear-signing-erc7730-registry`
//! CI contract at `.github/test-results/`. The `runner` field is the harness
//! category (literal `@ethereum-sourcify/clear-signing-test-runner`), and
//! `implementation` identifies the clear-signing build under test.

use std::path::Path;

use anyhow::{Context, Result};
use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel};
use indexmap::IndexMap;
use serde::Serialize;

use crate::compare::{first_failure_message, CaseResult};

/// Harness category literal expected by the registry CI.
pub const RUNNER_ID: &str = "@ethereum-sourcify/clear-signing-test-runner";

/// `implementation` field for the registry results: identifies the
/// clear-signing crate version this runner is bound to.
pub fn implementation_id() -> String {
    format!("llbartekll/clear-signing@{}", clear_signing::VERSION)
}

#[derive(Debug, Serialize)]
pub struct ResultsFile {
    pub runner: String,
    pub implementation: String,
    pub cases: Vec<CaseEntry>,
}

#[derive(Debug, Serialize)]
pub struct CaseEntry {
    pub description: String,
    pub status: CaseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<Rendered>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaseStatus {
    Pass,
    Fail,
    Error,
    Skipped,
}

#[derive(Debug, Serialize)]
pub struct Rendered {
    pub intent: String,
    pub owner: String,
    pub fields: IndexMap<String, FieldValue>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FieldValue {
    Value(String),
    Nested(NestedRendered),
}

#[derive(Debug, Serialize)]
pub struct NestedRendered {
    pub intent: String,
    pub owner: String,
    pub fields: IndexMap<String, FieldValue>,
}

pub fn build_results_file(results: &[CaseResult]) -> ResultsFile {
    ResultsFile {
        runner: RUNNER_ID.to_string(),
        implementation: implementation_id(),
        cases: results.iter().map(build_case_entry).collect(),
    }
}

fn build_case_entry(result: &CaseResult) -> CaseEntry {
    if let Some(err) = &result.error {
        return CaseEntry {
            description: result.description.clone(),
            status: CaseStatus::Error,
            rendered: None,
            message: Some(err.clone()),
        };
    }
    let rendered = result.model.as_ref().map(render_model);
    if result.passed {
        CaseEntry {
            description: result.description.clone(),
            status: CaseStatus::Pass,
            rendered,
            message: None,
        }
    } else {
        CaseEntry {
            description: result.description.clone(),
            status: CaseStatus::Fail,
            rendered,
            message: first_failure_message(result),
        }
    }
}

fn render_model(model: &DisplayModel) -> Rendered {
    Rendered {
        intent: model.intent.clone(),
        owner: model.owner.clone().unwrap_or_default(),
        fields: render_entries(&model.entries),
    }
}

fn render_entries(entries: &[DisplayEntry]) -> IndexMap<String, FieldValue> {
    let mut out: IndexMap<String, FieldValue> = IndexMap::new();
    for entry in entries {
        match entry {
            DisplayEntry::Item(DisplayItem { label, value }) => {
                out.insert(label.clone(), FieldValue::Value(value.clone()));
            }
            DisplayEntry::Group { items, .. } => {
                // Group fields are flattened to the surrounding map per the registry spec.
                for DisplayItem { label, value } in items {
                    out.insert(label.clone(), FieldValue::Value(value.clone()));
                }
            }
            DisplayEntry::Nested { label, intent, entries } => {
                out.insert(
                    label.clone(),
                    FieldValue::Nested(NestedRendered {
                        intent: intent.clone(),
                        // The engine does not currently track `owner` on nested
                        // calldata frames; emit empty so the registry schema
                        // stays well-formed.
                        owner: String::new(),
                        fields: render_entries(entries),
                    }),
                );
            }
        }
    }
    out
}

/// Atomic write: serialize and write to `<path>.tmp` then rename to `path`.
/// Avoids leaving a half-written results file if the runner crashes mid-write.
pub fn write_results_file(path: &Path, results_file: &ResultsFile) -> Result<()> {
    let body = serde_json::to_string_pretty(results_file)
        .context("serialize results.json")?;
    let tmp = tmp_path(path);
    std::fs::write(&tmp, &body)
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn tmp_path(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => std::path::PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel, GroupIteration};

    fn model_with(entries: Vec<DisplayEntry>) -> DisplayModel {
        DisplayModel {
            intent: "Outer intent".into(),
            interpolated_intent: None,
            entries,
            owner: None,
            contract_name: None,
        }
    }

    fn pass_result(model: DisplayModel) -> CaseResult {
        CaseResult {
            description: "t".into(),
            passed: true,
            failures: vec![],
            model: Some(model),
            error: None,
        }
    }

    #[test]
    fn group_entries_flatten_into_parent_fields() {
        let m = model_with(vec![DisplayEntry::Group {
            label: "g".into(),
            iteration: GroupIteration::Bundled,
            items: vec![
                DisplayItem { label: "A".into(), value: "1".into() },
                DisplayItem { label: "B".into(), value: "2".into() },
            ],
        }]);
        let r = pass_result(m);
        let file = build_results_file(std::slice::from_ref(&r));
        let rendered = file.cases[0].rendered.as_ref().unwrap();
        assert!(matches!(rendered.fields.get("A"), Some(FieldValue::Value(v)) if v == "1"));
        assert!(matches!(rendered.fields.get("B"), Some(FieldValue::Value(v)) if v == "2"));
        assert!(
            !rendered.fields.contains_key("g"),
            "group label must not appear as its own field"
        );
    }

    #[test]
    fn nested_calldata_emits_recursive_object() {
        let m = model_with(vec![DisplayEntry::Nested {
            label: "Inner call".into(),
            intent: "Transfer".into(),
            entries: vec![DisplayEntry::Item(DisplayItem {
                label: "To".into(),
                value: "0xabc".into(),
            })],
        }]);
        let r = pass_result(m);
        let file = build_results_file(std::slice::from_ref(&r));
        let rendered = file.cases[0].rendered.as_ref().unwrap();
        let nested = match rendered.fields.get("Inner call") {
            Some(FieldValue::Nested(n)) => n,
            other => panic!("expected nested, got {other:?}"),
        };
        assert_eq!(nested.intent, "Transfer");
        assert!(matches!(nested.fields.get("To"), Some(FieldValue::Value(v)) if v == "0xabc"));
    }
}
