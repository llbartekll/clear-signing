//! Registry-compatible `results.json` emission.
//!
//! Output format follows the CI contract at the ERC-7730 registry's
//! `.github/test-results/`. `runner` identifies this harness;
//! `implementation` identifies the clear-signing build it drives.
//!
//! `cases[].rendered.fields` is an **ordered array** of `{label, value}`
//! entries. Array-iteration descriptor paths (e.g. `signers.[]`) produce
//! multiple entries with the same label; collapsing them into a label-keyed
//! map would silently drop the duplicates, so the registry CI aggregator and
//! the runner both speak the array shape on this boundary.

use std::path::Path;

use anyhow::{Context, Result};
use clear_signing::engine::{DisplayEntry, DisplayItem, DisplayModel};
use serde::Serialize;

use crate::compare::{first_failure_message, CaseResult};

/// Identifier of this test runner harness.
pub const RUNNER_ID: &str = "@llbartekll/cs-test";

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
    #[serde(rename = "interpolatedIntent", skip_serializing_if = "Option::is_none")]
    pub interpolated_intent: Option<String>,
    pub owner: String,
    pub fields: Vec<FieldEntry>,
}

#[derive(Debug, Serialize)]
pub struct FieldEntry {
    pub label: String,
    pub value: FieldValue,
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
    pub fields: Vec<FieldEntry>,
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
        interpolated_intent: model.interpolated_intent.clone(),
        owner: model.owner.clone().unwrap_or_default(),
        fields: render_entries(&model.entries),
    }
}

fn render_entries(entries: &[DisplayEntry]) -> Vec<FieldEntry> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            DisplayEntry::Item(DisplayItem { label, value }) => {
                out.push(FieldEntry {
                    label: label.clone(),
                    value: FieldValue::Value(value.clone()),
                });
            }
            DisplayEntry::Group { items, .. } => {
                // Group fields flatten into the surrounding sequence per the registry spec.
                for DisplayItem { label, value } in items {
                    out.push(FieldEntry {
                        label: label.clone(),
                        value: FieldValue::Value(value.clone()),
                    });
                }
            }
            DisplayEntry::Nested {
                label,
                intent,
                owner,
                entries,
            } => {
                out.push(FieldEntry {
                    label: label.clone(),
                    value: FieldValue::Nested(NestedRendered {
                        intent: intent.clone(),
                        owner: owner.clone().unwrap_or_default(),
                        fields: render_entries(entries),
                    }),
                });
            }
        }
    }
    out
}

/// Atomic write: serialize and write to `<path>.tmp` then rename to `path`.
/// Avoids leaving a half-written results file if the runner crashes mid-write.
pub fn write_results_file(path: &Path, results_file: &ResultsFile) -> Result<()> {
    let body = serde_json::to_string_pretty(results_file).context("serialize results.json")?;
    let tmp = tmp_path(path);
    std::fs::write(&tmp, &body).with_context(|| format!("write {}", tmp.display()))?;
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
                DisplayItem {
                    label: "A".into(),
                    value: "1".into(),
                },
                DisplayItem {
                    label: "B".into(),
                    value: "2".into(),
                },
            ],
        }]);
        let r = pass_result(m);
        let file = build_results_file(std::slice::from_ref(&r));
        let rendered = file.cases[0].rendered.as_ref().unwrap();
        assert_eq!(rendered.fields.len(), 2);
        assert_eq!(rendered.fields[0].label, "A");
        assert!(matches!(&rendered.fields[0].value, FieldValue::Value(v) if v == "1"));
        assert_eq!(rendered.fields[1].label, "B");
        assert!(matches!(&rendered.fields[1].value, FieldValue::Value(v) if v == "2"));
        assert!(
            !rendered.fields.iter().any(|f| f.label == "g"),
            "group label must not appear as its own field"
        );
    }

    #[test]
    fn duplicate_labels_emit_separate_entries() {
        let m = model_with(vec![
            DisplayEntry::Item(DisplayItem {
                label: "Signer".into(),
                value: "0xaaa".into(),
            }),
            DisplayEntry::Item(DisplayItem {
                label: "Signer".into(),
                value: "0xbbb".into(),
            }),
        ]);
        let r = pass_result(m);
        let file = build_results_file(std::slice::from_ref(&r));
        let rendered = file.cases[0].rendered.as_ref().unwrap();
        assert_eq!(rendered.fields.len(), 2);
        assert_eq!(rendered.fields[0].label, "Signer");
        assert!(matches!(&rendered.fields[0].value, FieldValue::Value(v) if v == "0xaaa"));
        assert_eq!(rendered.fields[1].label, "Signer");
        assert!(matches!(&rendered.fields[1].value, FieldValue::Value(v) if v == "0xbbb"));
    }

    #[test]
    fn interpolated_intent_serializes_when_present() {
        let mut m = model_with(vec![]);
        m.interpolated_intent = Some("Swap 100 USDC for 99.5 DAI".into());
        let r = pass_result(m);
        let file = build_results_file(std::slice::from_ref(&r));
        let body = serde_json::to_string(&file).unwrap();
        assert!(
            body.contains("\"interpolatedIntent\":\"Swap 100 USDC for 99.5 DAI\""),
            "interpolatedIntent missing from output: {body}"
        );
    }

    #[test]
    fn interpolated_intent_omitted_when_absent() {
        let r = pass_result(model_with(vec![]));
        let file = build_results_file(std::slice::from_ref(&r));
        let body = serde_json::to_string(&file).unwrap();
        assert!(
            !body.contains("interpolatedIntent"),
            "interpolatedIntent should be omitted, got: {body}"
        );
    }

    #[test]
    fn nested_calldata_emits_recursive_object() {
        let m = model_with(vec![DisplayEntry::Nested {
            label: "Inner call".into(),
            intent: "Transfer".into(),
            owner: Some("Inner DAO".into()),
            entries: vec![DisplayEntry::Item(DisplayItem {
                label: "To".into(),
                value: "0xabc".into(),
            })],
        }]);
        let r = pass_result(m);
        let file = build_results_file(std::slice::from_ref(&r));
        let rendered = file.cases[0].rendered.as_ref().unwrap();
        assert_eq!(rendered.fields.len(), 1);
        assert_eq!(rendered.fields[0].label, "Inner call");
        let nested = match &rendered.fields[0].value {
            FieldValue::Nested(n) => n,
            other => panic!("expected nested, got {other:?}"),
        };
        assert_eq!(nested.intent, "Transfer");
        assert_eq!(nested.owner, "Inner DAO");
        assert_eq!(nested.fields.len(), 1);
        assert_eq!(nested.fields[0].label, "To");
        assert!(matches!(&nested.fields[0].value, FieldValue::Value(v) if v == "0xabc"));
    }
}
