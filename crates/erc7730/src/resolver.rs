//! Pluggable descriptor resolution via the [`DescriptorSource`] trait.
//! Includes [`StaticSource`] for testing and embedded use cases.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::error::ResolveError;
use crate::types::descriptor::Descriptor;

/// A resolved descriptor ready for use.
#[derive(Debug, Clone)]
pub struct ResolvedDescriptor {
    pub descriptor: Descriptor,
    pub chain_id: u64,
    pub address: String,
}

/// Trait for descriptor sources (embedded, filesystem, GitHub API, etc.).
pub trait DescriptorSource: Send + Sync {
    /// Resolve a descriptor for contract calldata clear signing.
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>>;

    /// Resolve a descriptor for EIP-712 typed data clear signing.
    fn resolve_typed(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>>;
}

/// Static in-memory descriptor source for testing.
pub struct StaticSource {
    /// Map of `"{chain_id}:{address}"` → Descriptor.
    calldata: HashMap<String, Descriptor>,
    typed: HashMap<String, Descriptor>,
}

impl StaticSource {
    pub fn new() -> Self {
        Self {
            calldata: HashMap::new(),
            typed: HashMap::new(),
        }
    }

    fn make_key(chain_id: u64, address: &str) -> String {
        format!("{}:{}", chain_id, address.to_lowercase())
    }

    /// Add a calldata descriptor.
    pub fn add_calldata(&mut self, chain_id: u64, address: &str, descriptor: Descriptor) {
        self.calldata
            .insert(Self::make_key(chain_id, address), descriptor);
    }

    /// Add a typed data descriptor.
    pub fn add_typed(&mut self, chain_id: u64, address: &str, descriptor: Descriptor) {
        self.typed
            .insert(Self::make_key(chain_id, address), descriptor);
    }

    /// Add a calldata descriptor from JSON.
    pub fn add_calldata_json(
        &mut self,
        chain_id: u64,
        address: &str,
        json: &str,
    ) -> Result<(), ResolveError> {
        let descriptor: Descriptor =
            serde_json::from_str(json).map_err(|e| ResolveError::Parse(e.to_string()))?;
        self.add_calldata(chain_id, address, descriptor);
        Ok(())
    }

    /// Add a typed data descriptor from JSON.
    pub fn add_typed_json(
        &mut self,
        chain_id: u64,
        address: &str,
        json: &str,
    ) -> Result<(), ResolveError> {
        let descriptor: Descriptor =
            serde_json::from_str(json).map_err(|e| ResolveError::Parse(e.to_string()))?;
        self.add_typed(chain_id, address, descriptor);
        Ok(())
    }
}

impl Default for StaticSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DescriptorSource for StaticSource {
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let key = Self::make_key(chain_id, address);
        let result = self
            .calldata
            .get(&key)
            .cloned()
            .map(|descriptor| ResolvedDescriptor {
                descriptor,
                chain_id,
                address: address.to_lowercase(),
            })
            .ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: address.to_string(),
            });
        Box::pin(async move { result })
    }

    fn resolve_typed(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let key = Self::make_key(chain_id, address);
        let result = self
            .typed
            .get(&key)
            .cloned()
            .map(|descriptor| ResolvedDescriptor {
                descriptor,
                chain_id,
                address: address.to_lowercase(),
            })
            .ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: address.to_string(),
            });
        Box::pin(async move { result })
    }
}

/// HTTP-based descriptor source that fetches from a GitHub registry.
///
/// Requires the `github-registry` feature.
#[cfg(feature = "github-registry")]
pub struct GitHubRegistrySource {
    base_url: String,
    /// Maps "{chain_id}:{address_lowercase}" → relative path in registry
    index: HashMap<String, String>,
    /// In-memory descriptor cache (tokio Mutex for async safety)
    cache: tokio::sync::Mutex<HashMap<String, Descriptor>>,
}

#[cfg(feature = "github-registry")]
impl GitHubRegistrySource {
    /// Create a new source with a manually provided index.
    ///
    /// `base_url`: raw content URL prefix (e.g., `"https://raw.githubusercontent.com/org/repo/main"`).
    /// `index`: maps `"{chain_id}:{address}"` → relative path (e.g., `"aave/calldata-lpv3.json"`).
    pub fn new(base_url: &str, index: HashMap<String, String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            index,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Create a source by fetching `index.json` from the registry.
    ///
    /// The index maps `"{chain_id}:{address_lowercase}"` → relative descriptor path.
    pub async fn from_registry(base_url: &str) -> Result<Self, ResolveError> {
        let base = base_url.trim_end_matches('/');
        let index_url = format!("{}/index.json", base);
        let response = reqwest::get(&index_url).await.map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                ResolveError::NotFound {
                    chain_id: 0,
                    address: format!("index.json at {index_url}"),
                }
            } else {
                ResolveError::Io(format!("HTTP fetch index failed: {e}"))
            }
        })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ResolveError::NotFound {
                chain_id: 0,
                address: format!("index.json at {index_url}"),
            });
        }
        let body = response
            .text()
            .await
            .map_err(|e| ResolveError::Io(format!("read index response: {e}")))?;
        let index: HashMap<String, String> =
            serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))?;
        Ok(Self::new(base, index))
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
        self.fetch_and_merge(rel_path, Self::MAX_INCLUDES_DEPTH)
            .await
    }

    /// Fetch a descriptor and recursively resolve `includes`.
    fn fetch_and_merge<'a>(
        &'a self,
        rel_path: &'a str,
        depth: u8,
    ) -> Pin<Box<dyn Future<Output = Result<Descriptor, ResolveError>> + Send + 'a>> {
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

                // Resolve relative URL against the including file's directory
                let resolved_path = resolve_relative_path(rel_path, &includes_path);
                let included = self.fetch_and_merge(&resolved_path, depth - 1).await?;
                let included_value = serde_json::to_value(&included)
                    .map_err(|e| ResolveError::Parse(e.to_string()))?;

                let merged = crate::merge::merge_descriptor_values(&value, &included_value);
                serde_json::from_value(merged).map_err(|e| ResolveError::Parse(e.to_string()))
            } else {
                serde_json::from_value(value).map_err(|e| ResolveError::Parse(e.to_string()))
            }
        })
    }
}

#[cfg(feature = "github-registry")]
impl DescriptorSource for GitHubRegistrySource {
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let address_owned = address.to_lowercase();
        Box::pin(async move {
            let key = Self::make_key(chain_id, &address_owned);

            // Check cache first
            if let Some(cached) = self.cache.lock().await.get(&key) {
                return Ok(ResolvedDescriptor {
                    descriptor: cached.clone(),
                    chain_id,
                    address: address_owned,
                });
            }

            let rel_path = self.index.get(&key).ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: address_owned.clone(),
            })?;

            let descriptor = self.fetch_descriptor(rel_path).await?;
            self.cache.lock().await.insert(key, descriptor.clone());

            Ok(ResolvedDescriptor {
                descriptor,
                chain_id,
                address: address_owned,
            })
        })
    }

    fn resolve_typed(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        self.resolve_calldata(chain_id, address)
    }
}

/// Resolve a relative path against a base file path.
///
/// E.g., `resolve_relative_path("aave/calldata-lpv3.json", "./erc20.json")` → `"aave/erc20.json"`.
#[cfg(feature = "github-registry")]
fn resolve_relative_path(base: &str, relative: &str) -> String {
    let relative = relative.strip_prefix("./").unwrap_or(relative);

    // Find the directory of the base path
    let dir = if let Some(pos) = base.rfind('/') {
        &base[..pos]
    } else {
        ""
    };

    if dir.is_empty() {
        relative.to_string()
    } else {
        // Handle `../` segments
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

    #[tokio::test]
    async fn test_static_source_not_found() {
        let source = StaticSource::new();
        let result = source.resolve_calldata(1, "0xabc").await;
        assert!(result.is_err());
    }

    #[cfg(feature = "github-registry")]
    #[test]
    fn test_resolve_relative_path_same_dir() {
        assert_eq!(
            resolve_relative_path("aave/calldata-lpv3.json", "./erc20.json"),
            "aave/erc20.json"
        );
    }

    #[cfg(feature = "github-registry")]
    #[test]
    fn test_resolve_relative_path_parent_dir() {
        assert_eq!(
            resolve_relative_path("aave/v3/calldata.json", "../../ercs/erc20.json"),
            "ercs/erc20.json"
        );
    }

    #[cfg(feature = "github-registry")]
    #[test]
    fn test_resolve_relative_path_no_dir() {
        assert_eq!(
            resolve_relative_path("file.json", "./other.json"),
            "other.json"
        );
    }
}
