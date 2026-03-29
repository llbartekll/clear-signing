use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use crate::error::ResolveError;
use crate::types::descriptor::Descriptor;

use super::source::{DescriptorSource, ResolvedDescriptor, TypedDescriptorLookup};

/// HTTP-based descriptor source that fetches from a GitHub registry.
///
/// Requires the `github-registry` feature.
pub struct GitHubRegistrySource {
    base_url: String,
    /// Calldata index: `"eip155:{chainId}:{address}"` → single relative path.
    calldata_index: HashMap<String, String>,
    /// EIP-712 index: `"eip155:{chainId}:{address}"` → `primaryType` buckets.
    eip712_index: HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>,
    /// In-memory descriptor cache keyed by relative path (tokio Mutex for async safety).
    cache: tokio::sync::Mutex<HashMap<String, Descriptor>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Eip712IndexEntry {
    pub(crate) path: String,
    #[serde(rename = "encodeTypeHashes", default)]
    pub(crate) encode_type_hashes: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RegistryIndexV2 {
    calldata: HashMap<String, String>,
    eip712: HashMap<String, Vec<LegacyEip712IndexEntry>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LegacyEip712IndexEntry {
    #[serde(rename = "primaryType")]
    primary_type: String,
    path: String,
}

impl GitHubRegistrySource {
    /// Create a new source with manually provided indexes.
    ///
    /// `base_url`: raw content URL prefix (e.g., `"https://raw.githubusercontent.com/org/repo/main"`).
    /// `calldata_index`: maps `"eip155:{chainId}:{address}"` → single relative path.
    /// `eip712_index`: maps `"eip155:{chainId}:{address}"` → list of `Eip712IndexEntry`.
    pub fn new(
        base_url: &str,
        calldata_index: HashMap<String, String>,
        eip712_index: HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            calldata_index,
            eip712_index,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Create a source by fetching split V3 index files from the registry.
    ///
    /// Falls back to the legacy V2 `index.json` layout when the split files are absent.
    pub async fn from_registry(base_url: &str) -> Result<Self, ResolveError> {
        let base = base_url.trim_end_matches('/');
        let calldata_url = format!("{}/index.calldata.json", base);
        let eip712_url = format!("{}/index.eip712.json", base);

        let split_indexes = match (
            fetch_index::<HashMap<String, String>>(&calldata_url).await,
            fetch_index::<HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>>(&eip712_url)
                .await,
        ) {
            (Ok(calldata), Ok(eip712)) => Some((calldata, eip712)),
            (Err(ResolveError::NotFound { .. }), Err(ResolveError::NotFound { .. }))
            | (Err(ResolveError::NotFound { .. }), Ok(_))
            | (Ok(_), Err(ResolveError::NotFound { .. })) => None,
            (Err(err), _) | (_, Err(err)) => return Err(err),
        };

        let (calldata_index, eip712_index) = if let Some(indexes) = split_indexes {
            indexes
        } else {
            let index_url = format!("{}/index.json", base);
            let index_v2 = fetch_index::<RegistryIndexV2>(&index_url).await?;
            (
                index_v2.calldata,
                group_legacy_eip712_index(index_v2.eip712),
            )
        };

        Ok(Self {
            base_url: base.to_string(),
            calldata_index,
            eip712_index,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    fn make_key(chain_id: u64, address: &str) -> String {
        format!("eip155:{}:{}", chain_id, address.to_lowercase())
    }

    /// Maximum depth for nested `includes` resolution.
    const MAX_INCLUDES_DEPTH: u8 = 3;

    async fn fetch_raw(&self, rel_path: &str) -> Result<String, ResolveError> {
        let url = format!("{}/{}", self.base_url, rel_path);
        let response = reqwest::get(&url).await.map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                ResolveError::NotFound {
                    chain_id: 0,
                    address: format!("descriptor at {url}"),
                }
            } else {
                ResolveError::Io(format!("HTTP fetch failed: {e}"))
            }
        })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ResolveError::NotFound {
                chain_id: 0,
                address: format!("descriptor at {url}"),
            });
        }
        response
            .text()
            .await
            .map_err(|e| ResolveError::Io(format!("read response: {e}")))
    }

    async fn fetch_descriptor(&self, rel_path: &str) -> Result<Descriptor, ResolveError> {
        let value = self
            .fetch_and_merge_value(rel_path, Self::MAX_INCLUDES_DEPTH)
            .await?;
        serde_json::from_value::<Descriptor>(value).map_err(|e| ResolveError::Parse(e.to_string()))
    }

    /// Fetch a descriptor, checking the cache first.
    async fn fetch_descriptor_cached(&self, rel_path: &str) -> Result<Descriptor, ResolveError> {
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(rel_path) {
                return Ok(cached.clone());
            }
        }
        let descriptor = self.fetch_descriptor(rel_path).await?;
        self.cache
            .lock()
            .await
            .insert(rel_path.to_string(), descriptor.clone());
        Ok(descriptor)
    }

    /// Fetch a descriptor JSON and recursively resolve `includes`, returning
    /// the merged JSON value. Deserialization into [`Descriptor`] happens only
    /// at the top-level caller so that partial included files (which may lack
    /// required fields like `context`) don't cause parse errors.
    fn fetch_and_merge_value<'a>(
        &'a self,
        rel_path: &'a str,
        depth: u8,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, ResolveError>> + Send + 'a>> {
        Box::pin(async move {
            let body = self.fetch_raw(rel_path).await?;
            let value: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))?;

            let includes = value
                .as_object()
                .and_then(|o| o.get("includes"))
                .and_then(|v| v.as_str())
                .map(String::from);

            if let Some(includes_path) = includes {
                if depth == 0 {
                    return Err(ResolveError::Io(
                        "max includes depth exceeded (possible circular reference)".to_string(),
                    ));
                }

                let resolved_path = resolve_relative_path(rel_path, &includes_path);
                let included_value = self
                    .fetch_and_merge_value(&resolved_path, depth - 1)
                    .await?;

                Ok(crate::merge::merge_descriptor_values(
                    &value,
                    &included_value,
                ))
            } else {
                Ok(value)
            }
        })
    }

    async fn resolve_typed_candidates_inner(
        &self,
        lookup: &TypedDescriptorLookup,
    ) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
        let address_lower = lookup.verifying_contract.to_lowercase();
        let key = Self::make_key(lookup.chain_id, &address_lower);
        let entries = self
            .eip712_index
            .get(&key)
            .and_then(|bucket| bucket.get(&lookup.primary_type))
            .ok_or_else(|| ResolveError::NotFound {
                chain_id: lookup.chain_id,
                address: address_lower.clone(),
            })?;

        let filtered_entries =
            filter_typed_index_entries(entries, lookup.encode_type_hash.as_deref());
        if filtered_entries.is_empty() {
            return Err(ResolveError::NotFound {
                chain_id: lookup.chain_id,
                address: address_lower,
            });
        }

        let mut seen_paths = HashSet::new();
        let mut candidates = Vec::new();
        for entry in filtered_entries {
            if !seen_paths.insert(entry.path.as_str()) {
                continue;
            }
            let descriptor = self.fetch_descriptor_cached(&entry.path).await?;
            candidates.push(ResolvedDescriptor {
                descriptor,
                chain_id: lookup.chain_id,
                address: lookup.verifying_contract.to_lowercase(),
            });
        }
        Ok(candidates)
    }
}

impl DescriptorSource for GitHubRegistrySource {
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let addr = address.to_lowercase();
        Box::pin(async move {
            let key = Self::make_key(chain_id, &addr);
            let path = self
                .calldata_index
                .get(&key)
                .ok_or_else(|| ResolveError::NotFound {
                    chain_id,
                    address: addr.clone(),
                })?;
            let descriptor = self.fetch_descriptor_cached(path).await?;
            Ok(ResolvedDescriptor {
                descriptor,
                chain_id,
                address: addr,
            })
        })
    }

    fn resolve_typed_candidates(
        &self,
        lookup: TypedDescriptorLookup,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResolvedDescriptor>, ResolveError>> + Send + '_>>
    {
        Box::pin(async move { self.resolve_typed_candidates_inner(&lookup).await })
    }
}

async fn fetch_index<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, ResolveError> {
    let response = reqwest::get(url).await.map_err(|e| {
        if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
            ResolveError::NotFound {
                chain_id: 0,
                address: format!("index at {url}"),
            }
        } else {
            ResolveError::Io(format!("HTTP fetch index failed: {e}"))
        }
    })?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ResolveError::NotFound {
            chain_id: 0,
            address: format!("index at {url}"),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|e| ResolveError::Io(format!("read index response: {e}")))?;
    serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))
}

fn group_legacy_eip712_index(
    legacy: HashMap<String, Vec<LegacyEip712IndexEntry>>,
) -> HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>> {
    let mut grouped = HashMap::new();

    for (key, entries) in legacy {
        let primary_map = grouped.entry(key).or_insert_with(HashMap::new);
        for entry in entries {
            let path = entry.path;
            let bucket: &mut Vec<Eip712IndexEntry> = primary_map
                .entry(entry.primary_type)
                .or_insert_with(Vec::new);
            if bucket.iter().any(|existing| existing.path == path) {
                continue;
            }
            bucket.push(Eip712IndexEntry {
                path,
                encode_type_hashes: Vec::new(),
            });
        }
    }

    grouped
}

fn filter_typed_index_entries<'a>(
    entries: &'a [Eip712IndexEntry],
    expected_hash: Option<&str>,
) -> Vec<&'a Eip712IndexEntry> {
    match expected_hash {
        Some(expected_hash) => {
            let exact = entries
                .iter()
                .filter(|entry| {
                    entry
                        .encode_type_hashes
                        .iter()
                        .any(|hash| hash.eq_ignore_ascii_case(expected_hash))
                })
                .collect::<Vec<_>>();
            if exact.is_empty() {
                if entries
                    .iter()
                    .all(|entry| entry.encode_type_hashes.is_empty())
                {
                    entries.iter().collect()
                } else {
                    Vec::new()
                }
            } else {
                exact
            }
        }
        None => entries.iter().collect(),
    }
}

/// Resolve a relative path against a base file path.
///
/// E.g., `resolve_relative_path("aave/calldata-lpv3.json", "./erc20.json")` → `"aave/erc20.json"`.
fn resolve_relative_path(base: &str, relative: &str) -> String {
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
    use std::collections::HashMap;

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
    fn test_group_legacy_eip712_index_groups_by_primary_type() {
        let grouped = group_legacy_eip712_index(HashMap::from([(
            "eip155:1:0xabc".to_string(),
            vec![
                LegacyEip712IndexEntry {
                    primary_type: "PermitSingle".to_string(),
                    path: "registry/uniswap/eip712-uniswap-permit2.json".to_string(),
                },
                LegacyEip712IndexEntry {
                    primary_type: "PermitBatch".to_string(),
                    path: "registry/uniswap/eip712-uniswap-permit2.json".to_string(),
                },
            ],
        )]));

        let bucket = grouped.get("eip155:1:0xabc").expect("bucket");
        assert_eq!(
            bucket["PermitSingle"][0].path,
            "registry/uniswap/eip712-uniswap-permit2.json"
        );
        assert_eq!(
            bucket["PermitSingle"][0].encode_type_hashes,
            Vec::<String>::new()
        );
        assert_eq!(
            bucket["PermitBatch"][0].path,
            "registry/uniswap/eip712-uniswap-permit2.json"
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
    fn test_filter_typed_index_entries_keeps_legacy_bucket_fallback() {
        let entries = vec![
            Eip712IndexEntry {
                path: "registry/legacy-a.json".to_string(),
                encode_type_hashes: Vec::new(),
            },
            Eip712IndexEntry {
                path: "registry/legacy-b.json".to_string(),
                encode_type_hashes: Vec::new(),
            },
        ];

        let filtered = filter_typed_index_entries(&entries, Some("0xaaaa"));
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].path, "registry/legacy-a.json");
        assert_eq!(filtered[1].path, "registry/legacy-b.json");
    }
}
