//! Pure helpers shared by registry-backed descriptor sources
//! (`github_registry`, `bundled_registry`): index entry types, typed-index
//! filtering, and relative-path resolution. No transport concerns here.

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Eip712IndexEntry {
    pub(crate) path: String,
    #[serde(rename = "encodeTypeHashes", default)]
    pub(crate) encode_type_hashes: Vec<String>,
}

pub(crate) fn filter_typed_index_entries<'a>(
    entries: &'a [Eip712IndexEntry],
    expected_hash: Option<&str>,
) -> Vec<&'a Eip712IndexEntry> {
    match expected_hash {
        Some(expected_hash) => entries
            .iter()
            .filter(|entry| {
                entry
                    .encode_type_hashes
                    .iter()
                    .any(|hash| hash.eq_ignore_ascii_case(expected_hash))
            })
            .collect::<Vec<_>>(),
        None => entries.iter().collect(),
    }
}

/// Resolve a relative path against a base file path.
///
/// E.g., `resolve_relative_path("aave/calldata-lpv3.json", "./erc20.json")` → `"aave/erc20.json"`.
pub(crate) fn resolve_relative_path(base: &str, relative: &str) -> String {
    let relative = relative.strip_prefix("./").unwrap_or(relative);

    let dir = if let Some(pos) = base.rfind('/') {
        &base[..pos]
    } else {
        ""
    };

    if dir.is_empty() {
        relative.to_string()
    } else {
        let mut parts: Vec<&str> = dir.split('/').collect();
        let mut rel_remaining = relative;
        while let Some(rest) = rel_remaining.strip_prefix("../") {
            parts.pop();
            rel_remaining = rest;
        }
        if parts.is_empty() {
            rel_remaining.to_string()
        } else {
            format!("{}/{}", parts.join("/"), rel_remaining)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_relative_path_same_dir() {
        assert_eq!(
            resolve_relative_path("aave/calldata-lpv3.json", "./erc20.json"),
            "aave/erc20.json"
        );
    }

    #[test]
    fn test_resolve_relative_path_parent_dir() {
        assert_eq!(
            resolve_relative_path("aave/v3/calldata.json", "../../ercs/erc20.json"),
            "ercs/erc20.json"
        );
    }

    #[test]
    fn test_resolve_relative_path_no_dir() {
        assert_eq!(
            resolve_relative_path("file.json", "./other.json"),
            "other.json"
        );
    }

    #[test]
    fn test_filter_typed_index_entries_requires_exact_hash_for_split_entries() {
        let entries = vec![
            Eip712IndexEntry {
                path: "registry/a.json".to_string(),
                encode_type_hashes: vec!["0xaaaa".to_string()],
            },
            Eip712IndexEntry {
                path: "registry/legacy.json".to_string(),
                encode_type_hashes: Vec::new(),
            },
        ];

        let filtered = filter_typed_index_entries(&entries, Some("0xaaaa"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, "registry/a.json");

        let no_match = filter_typed_index_entries(&entries, Some("0xbbbb"));
        assert!(no_match.is_empty());
    }

    #[test]
    fn test_filter_typed_index_entries_rejects_empty_hash_entries() {
        let entries = vec![
            Eip712IndexEntry {
                path: "registry/a.json".to_string(),
                encode_type_hashes: Vec::new(),
            },
            Eip712IndexEntry {
                path: "registry/b.json".to_string(),
                encode_type_hashes: Vec::new(),
            },
        ];

        let filtered = filter_typed_index_entries(&entries, Some("0xaaaa"));
        assert!(filtered.is_empty());
    }
}
