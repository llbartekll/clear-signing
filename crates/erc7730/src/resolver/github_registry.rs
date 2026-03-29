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

    /// Create a source by fetching the required split V3 index files from the registry.
    pub async fn from_registry(base_url: &str) -> Result<Self, ResolveError> {
        let base = base_url.trim_end_matches('/');
        let calldata_url = format!("{}/index.calldata.json", base);
        let eip712_url = format!("{}/index.eip712.json", base);
        let calldata_index = fetch_index::<HashMap<String, String>>(&calldata_url).await?;
        let eip712_index =
            fetch_index::<HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>>(&eip712_url)
                .await?;

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
        let response = reqwest::get(&url)
            .await
            .map_err(|e| ResolveError::RegistryIo(format!("HTTP fetch failed for {url}: {e}")))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ResolveError::RegistryDescriptorMissing { url });
        }
        response
            .text()
            .await
            .map_err(|e| ResolveError::RegistryIo(format!("read response for {url}: {e}")))
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
                    return Err(ResolveError::RegistryIo(
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
    let response = reqwest::get(url)
        .await
        .map_err(|e| ResolveError::RegistryIo(format!("HTTP fetch index failed for {url}: {e}")))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ResolveError::RegistryIndexMissing {
            url: url.to_string(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|e| ResolveError::RegistryIo(format!("read index response for {url}: {e}")))?;
    serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))
}

fn filter_typed_index_entries<'a>(
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn spawn_test_server(
        routes: Vec<(&'static str, u16, &'static str)>,
        requests: usize,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let handle = thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    let n = stream.read(&mut buf).expect("read");
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }

                let request = String::from_utf8_lossy(&request);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let (status, body) = routes
                    .iter()
                    .find(|(route, _, _)| *route == path)
                    .map(|(_, status, body)| (*status, *body))
                    .unwrap_or((404, ""));
                let status_text = match status {
                    200 => "OK",
                    404 => "Not Found",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
        });
        (format!("http://{}", addr), handle)
    }

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

    #[tokio::test]
    async fn test_from_registry_requires_split_indexes() {
        let (base_url, handle) = spawn_test_server(
            vec![
                ("/index.calldata.json", 404, ""),
                ("/index.eip712.json", 200, "{}"),
            ],
            1,
        );

        let err = match GitHubRegistrySource::from_registry(&base_url).await {
            Ok(_) => panic!("missing split index should fail"),
            Err(err) => err,
        };
        match err {
            ResolveError::RegistryIndexMissing { url } => {
                assert!(url.ends_with("/index.calldata.json"));
            }
            other => panic!("expected RegistryIndexMissing, got {other:?}"),
        }

        handle.join().expect("server join");
    }

    #[tokio::test]
    async fn test_resolve_calldata_reports_missing_descriptor_file() {
        let (base_url, handle) = spawn_test_server(vec![("/registry/missing.json", 404, "")], 1);

        let source = GitHubRegistrySource::new(
            &base_url,
            HashMap::from([(
                "eip155:1:0xabc".to_string(),
                "registry/missing.json".to_string(),
            )]),
            HashMap::new(),
        );

        let err = source
            .resolve_calldata(1, "0xabc")
            .await
            .expect_err("missing descriptor file should fail");
        match err {
            ResolveError::RegistryDescriptorMissing { url } => {
                assert!(url.ends_with("/registry/missing.json"));
            }
            other => panic!("expected RegistryDescriptorMissing, got {other:?}"),
        }

        handle.join().expect("server join");
    }
}
