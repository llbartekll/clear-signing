//! JSON-level merge for ERC-7730 `includes` mechanism.
//!
//! Merges an including descriptor with its included descriptor at the
//! `serde_json::Value` level — before deserialization into [`Descriptor`].
//! The including file takes precedence on conflicts. Field arrays merge by `path`.

use serde_json::{Map, Value};

use crate::error::Error;

/// Merge two descriptor JSON strings (including + included).
///
/// Returns merged JSON ready for [`Descriptor::from_json()`].
/// The including file's values take precedence. `display.formats[].fields`
/// arrays are merged by matching `path` keys.
pub fn merge_descriptors(including_json: &str, included_json: &str) -> Result<String, Error> {
    let including: Value = serde_json::from_str(including_json)
        .map_err(|e| Error::Descriptor(format!("invalid including JSON: {e}")))?;
    let included: Value = serde_json::from_str(included_json)
        .map_err(|e| Error::Descriptor(format!("invalid included JSON: {e}")))?;
    let merged = merge_descriptor_values(&including, &included);
    serde_json::to_string_pretty(&merged)
        .map_err(|e| Error::Descriptor(format!("failed to serialize merged descriptor: {e}")))
}

/// Merge an including descriptor JSON value with its included descriptor JSON value.
///
/// Start with included as base, overlay with including (including wins conflicts).
/// The `includes` key is stripped from the result.
pub fn merge_descriptor_values(including: &Value, included: &Value) -> Value {
    let (Some(inc_obj), Some(base_obj)) = (including.as_object(), included.as_object()) else {
        // If either isn't an object, including wins
        return including.clone();
    };

    let mut result = base_obj.clone();

    for (key, inc_val) in inc_obj {
        // Strip `includes` — it's consumed, not propagated
        if key == "includes" {
            continue;
        }

        match key.as_str() {
            "display" => {
                let base_display = result.get("display").cloned().unwrap_or(Value::Null);
                result.insert(key.clone(), merge_display(&base_display, inc_val));
            }
            "metadata" => {
                let base_meta = result.get("metadata").cloned().unwrap_or(Value::Null);
                result.insert(key.clone(), merge_metadata(&base_meta, inc_val));
            }
            "context" => {
                let base_ctx = result.get("context").cloned().unwrap_or(Value::Null);
                if let (Some(b), Some(o)) = (base_ctx.as_object(), inc_val.as_object()) {
                    result.insert(key.clone(), Value::Object(deep_merge_objects(b, o)));
                } else {
                    result.insert(key.clone(), inc_val.clone());
                }
            }
            _ => {
                // $schema, $id, and any unknown keys — including wins
                result.insert(key.clone(), inc_val.clone());
            }
        }
    }

    // Strip `includes` from base too (in case included file also had it — already resolved)
    result.remove("includes");

    Value::Object(result)
}

/// Merge display objects. Both may be absent/null.
fn merge_display(base: &Value, over: &Value) -> Value {
    if base.is_null() {
        return over.clone();
    }
    if over.is_null() {
        return base.clone();
    }

    let (Some(base_obj), Some(over_obj)) = (base.as_object(), over.as_object()) else {
        return over.clone();
    };

    let mut result = base_obj.clone();

    // Merge "definitions" as objects (over wins per key)
    if let Some(over_defs) = over_obj.get("definitions") {
        let base_defs = result.get("definitions").cloned().unwrap_or(Value::Null);
        if let (Some(b), Some(o)) = (base_defs.as_object(), over_defs.as_object()) {
            let mut merged = b.clone();
            for (k, v) in o {
                merged.insert(k.clone(), v.clone());
            }
            result.insert("definitions".to_string(), Value::Object(merged));
        } else {
            result.insert("definitions".to_string(), over_defs.clone());
        }
    }

    // Merge "formats" by function signature key
    if let Some(over_formats) = over_obj.get("formats") {
        let base_formats = result.get("formats").cloned().unwrap_or(Value::Null);
        if let (Some(b), Some(o)) = (base_formats.as_object(), over_formats.as_object()) {
            result.insert("formats".to_string(), Value::Object(merge_formats(b, o)));
        } else {
            result.insert("formats".to_string(), over_formats.clone());
        }
    }

    // Copy any other display keys from over (over wins)
    for (k, v) in over_obj {
        if k != "definitions" && k != "formats" {
            result.insert(k.clone(), v.clone());
        }
    }

    Value::Object(result)
}

/// Merge format maps by function signature key.
fn merge_formats(base: &Map<String, Value>, over: &Map<String, Value>) -> Map<String, Value> {
    let mut result = base.clone();

    for (key, over_fmt) in over {
        if let Some(base_fmt) = result.get(key).cloned() {
            result.insert(key.clone(), merge_format(&base_fmt, over_fmt));
        } else {
            result.insert(key.clone(), over_fmt.clone());
        }
    }

    result
}

/// Merge a single format object (intent, fields, etc.).
fn merge_format(base: &Value, over: &Value) -> Value {
    let (Some(base_obj), Some(over_obj)) = (base.as_object(), over.as_object()) else {
        return over.clone();
    };

    let mut result = base_obj.clone();

    for (key, over_val) in over_obj {
        if key == "fields" {
            let base_fields = result.get("fields").and_then(|v| v.as_array());
            let over_fields = over_val.as_array();
            if let (Some(bf), Some(of)) = (base_fields, over_fields) {
                result.insert("fields".to_string(), Value::Array(merge_fields(bf, of)));
            } else {
                result.insert(key.clone(), over_val.clone());
            }
        } else {
            // intent, interpolatedIntent, $id, excluded — over wins
            result.insert(key.clone(), over_val.clone());
        }
    }

    Value::Object(result)
}

/// Merge field arrays by `path` key. Fields without a matching path are appended.
fn merge_fields(base: &[Value], over: &[Value]) -> Vec<Value> {
    let mut result: Vec<Value> = base.to_vec();

    for over_field in over {
        let over_path = over_field
            .as_object()
            .and_then(|o| o.get("path"))
            .and_then(|v| v.as_str());

        if let Some(path) = over_path {
            // Find matching field in result by path
            let found = result.iter_mut().find(|f| {
                f.as_object()
                    .and_then(|o| o.get("path"))
                    .and_then(|v| v.as_str())
                    == Some(path)
            });

            if let Some(existing) = found {
                // Deep merge: over wins per key, params deep-merged
                if let (Some(base_obj), Some(over_obj)) =
                    (existing.as_object(), over_field.as_object())
                {
                    *existing = Value::Object(deep_merge_objects(base_obj, over_obj));
                } else {
                    *existing = over_field.clone();
                }
            } else {
                result.push(over_field.clone());
            }
        } else {
            // No path (e.g., fieldGroup) — append
            result.push(over_field.clone());
        }
    }

    result
}

/// Merge metadata objects. Nested collections (enums, constants, maps) are
/// merged per-key; scalar keys (owner, contractName, info, token) are replaced.
fn merge_metadata(base: &Value, over: &Value) -> Value {
    if base.is_null() {
        return over.clone();
    }
    if over.is_null() {
        return base.clone();
    }

    let (Some(base_obj), Some(over_obj)) = (base.as_object(), over.as_object()) else {
        return over.clone();
    };

    let mut result = base_obj.clone();
    let collection_keys = ["enums", "constants", "maps", "addressBook"];

    for (key, over_val) in over_obj {
        if collection_keys.contains(&key.as_str()) {
            // Merge sub-object keys (over wins per key)
            let base_sub = result.get(key).cloned().unwrap_or(Value::Null);
            if let (Some(b), Some(o)) = (base_sub.as_object(), over_val.as_object()) {
                let mut merged = b.clone();
                for (k, v) in o {
                    merged.insert(k.clone(), v.clone());
                }
                result.insert(key.clone(), Value::Object(merged));
            } else {
                result.insert(key.clone(), over_val.clone());
            }
        } else {
            // Scalar replacement: owner, contractName, info, token
            result.insert(key.clone(), over_val.clone());
        }
    }

    Value::Object(result)
}

/// Recursively deep-merge two JSON objects. `over` wins on leaf conflicts.
fn deep_merge_objects(base: &Map<String, Value>, over: &Map<String, Value>) -> Map<String, Value> {
    let mut result = base.clone();

    for (key, over_val) in over {
        if let Some(base_val) = result.get(key) {
            if let (Some(b), Some(o)) = (base_val.as_object(), over_val.as_object()) {
                result.insert(key.clone(), Value::Object(deep_merge_objects(b, o)));
            } else {
                result.insert(key.clone(), over_val.clone());
            }
        } else {
            result.insert(key.clone(), over_val.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_empty_fields() {
        let base = vec![];
        let over = vec![serde_json::json!({"path": "a", "label": "A"})];
        let result = merge_fields(&base, &over);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["path"], "a");
    }

    #[test]
    fn test_merge_fields_no_overlap() {
        let base = vec![serde_json::json!({"path": "a", "label": "A"})];
        let over = vec![serde_json::json!({"path": "b", "label": "B"})];
        let result = merge_fields(&base, &over);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["path"], "a");
        assert_eq!(result[1]["path"], "b");
    }

    #[test]
    fn test_merge_fields_by_path_override() {
        let base = vec![serde_json::json!({
            "path": "amount", "label": "Amount", "format": "tokenAmount",
            "params": { "tokenPath": "@.to", "threshold": "0x800" }
        })];
        let over = vec![serde_json::json!({
            "path": "amount",
            "params": { "threshold": "0xFFF" }
        })];
        let result = merge_fields(&base, &over);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["label"], "Amount");
        assert_eq!(result[0]["format"], "tokenAmount");
        assert_eq!(result[0]["params"]["tokenPath"], "@.to");
        assert_eq!(result[0]["params"]["threshold"], "0xFFF");
    }

    #[test]
    fn test_deep_merge_nested_params() {
        let base: Map<String, Value> =
            serde_json::from_str(r#"{"params": {"a": 1, "b": 2, "nested": {"x": 10}}}"#).unwrap();
        let over: Map<String, Value> =
            serde_json::from_str(r#"{"params": {"b": 99, "nested": {"y": 20}}}"#).unwrap();
        let result = deep_merge_objects(&base, &over);
        assert_eq!(result["params"]["a"], 1);
        assert_eq!(result["params"]["b"], 99);
        assert_eq!(result["params"]["nested"]["x"], 10);
        assert_eq!(result["params"]["nested"]["y"], 20);
    }

    #[test]
    fn test_merge_display_missing_base() {
        let base = Value::Null;
        let over = serde_json::json!({"formats": {"foo()": {"intent": "Foo"}}});
        let result = merge_display(&base, &over);
        assert_eq!(result["formats"]["foo()"]["intent"], "Foo");
    }

    #[test]
    fn test_merge_display_missing_over() {
        let base = serde_json::json!({"formats": {"foo()": {"intent": "Foo"}}});
        let over = Value::Null;
        let result = merge_display(&base, &over);
        assert_eq!(result["formats"]["foo()"]["intent"], "Foo");
    }

    #[test]
    fn test_merge_metadata_collection_merge() {
        let base = serde_json::json!({
            "owner": "Original",
            "enums": { "modeA": {"0": "Off", "1": "On"} }
        });
        let over = serde_json::json!({
            "owner": "Override",
            "enums": { "modeB": {"0": "Low", "1": "High"} }
        });
        let result = merge_metadata(&base, &over);
        assert_eq!(result["owner"], "Override");
        assert!(result["enums"]["modeA"].is_object());
        assert!(result["enums"]["modeB"].is_object());
    }

    #[test]
    fn test_merge_descriptor_strips_includes() {
        let including = serde_json::json!({
            "includes": "./base.json",
            "metadata": {"owner": "Override"}
        });
        let included = serde_json::json!({
            "metadata": {"owner": "Base"}
        });
        let result = merge_descriptor_values(&including, &included);
        assert!(result.get("includes").is_none());
        assert_eq!(result["metadata"]["owner"], "Override");
    }

    #[test]
    fn test_merge_descriptors_full() {
        let included = r#"{
            "context": {
                "contract": { "abi": ["function transfer(address,uint256)"] }
            },
            "display": {
                "definitions": {},
                "formats": {
                    "transfer(address,uint256)": {
                        "intent": "Transfer",
                        "fields": [
                            {"path": "to", "label": "Recipient", "format": "addressName"},
                            {"path": "amount", "label": "Amount", "format": "tokenAmount",
                             "params": {"tokenPath": "@.to", "threshold": "0x800"}}
                        ]
                    }
                }
            }
        }"#;

        let including = r#"{
            "includes": "./base.json",
            "context": {
                "contract": {
                    "deployments": [{"chainId": 1, "address": "0xdAC17"}]
                }
            },
            "metadata": {"owner": "Tether", "contractName": "USDT"},
            "display": {
                "formats": {
                    "transfer(address,uint256)": {
                        "fields": [
                            {"path": "amount", "params": {"threshold": "0xFFF"}}
                        ]
                    }
                }
            }
        }"#;

        let result_json = merge_descriptors(including, included).unwrap();
        let result: Value = serde_json::from_str(&result_json).unwrap();

        // includes stripped
        assert!(result.get("includes").is_none());
        // context merged: both abi and deployments present
        assert!(result["context"]["contract"]["abi"].is_array());
        assert!(result["context"]["contract"]["deployments"].is_array());
        // metadata from including
        assert_eq!(result["metadata"]["owner"], "Tether");
        // fields merged: both fields present, threshold overridden
        let fields = result["display"]["formats"]["transfer(address,uint256)"]["fields"]
            .as_array()
            .unwrap();
        assert_eq!(fields.len(), 2);
        // First field preserved from included
        assert_eq!(fields[0]["path"], "to");
        assert_eq!(fields[0]["label"], "Recipient");
        // Second field: threshold overridden, tokenPath preserved
        assert_eq!(fields[1]["path"], "amount");
        assert_eq!(fields[1]["params"]["threshold"], "0xFFF");
        assert_eq!(fields[1]["params"]["tokenPath"], "@.to");
    }
}
