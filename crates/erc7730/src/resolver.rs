//! Pluggable descriptor resolution via the [`DescriptorSource`] trait.
//! Includes [`StaticSource`] for testing and embedded use cases.

use std::collections::HashMap;
#[cfg(feature = "github-registry")]
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use crate::error::{Error, ResolveError};
use crate::types::descriptor::Descriptor;

/// A resolved descriptor ready for use.
#[derive(Debug, Clone)]
pub struct ResolvedDescriptor {
    pub descriptor: Descriptor,
    pub chain_id: u64,
    pub address: String,
}

/// Lookup parameters for EIP-712 descriptor resolution.
#[derive(Debug, Clone)]
pub struct TypedDescriptorLookup {
    pub chain_id: u64,
    pub verifying_contract: String,
    pub primary_type: String,
    pub encode_type_hash: Option<String>,
}

/// Trait for descriptor sources (embedded, filesystem, GitHub API, etc.).
pub trait DescriptorSource: Send + Sync {
    /// Resolve a descriptor for contract calldata clear signing.
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>>;

    /// Resolve candidate descriptors for EIP-712 typed data clear signing.
    fn resolve_typed_candidates(
        &self,
        lookup: TypedDescriptorLookup,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResolvedDescriptor>, ResolveError>> + Send + '_>>;
}

/// Static in-memory descriptor source for testing.
pub struct StaticSource {
    /// Map of `"{chain_id}:{address}"` → Descriptor.
    calldata: HashMap<String, Descriptor>,
    typed: HashMap<String, Vec<Descriptor>>,
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
            .entry(Self::make_key(chain_id, address))
            .or_default()
            .push(descriptor);
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

    fn resolve_typed_candidates(
        &self,
        lookup: TypedDescriptorLookup,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResolvedDescriptor>, ResolveError>> + Send + '_>>
    {
        let key = Self::make_key(lookup.chain_id, &lookup.verifying_contract);
        let address_lower = lookup.verifying_contract.to_lowercase();
        let primary_type = lookup.primary_type.clone();
        let expected_hash = lookup.encode_type_hash.clone();
        let result = self
            .typed
            .get(&key)
            .map(|descriptors| {
                descriptors
                    .iter()
                    .filter(|descriptor| {
                        descriptor.display.formats.keys().any(|key| {
                            let primary_matches = key == &primary_type
                                || key.strip_prefix(&format!("{primary_type}(")).is_some();
                            if !primary_matches {
                                return false;
                            }

                            match expected_hash.as_deref() {
                                Some(expected) if key.contains('(') => {
                                    crate::eip712::format_key_hash_hex(key)
                                        .eq_ignore_ascii_case(expected)
                                }
                                _ => true,
                            }
                        })
                    })
                    .cloned()
                    .map(|descriptor| ResolvedDescriptor {
                        descriptor,
                        chain_id: lookup.chain_id,
                        address: address_lower.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let result = if result.is_empty() {
            Err(ResolveError::NotFound {
                chain_id: lookup.chain_id,
                address: lookup.verifying_contract.clone(),
            })
        } else {
            Ok(result)
        };
        Box::pin(async move { result })
    }
}

/// HTTP-based descriptor source that fetches from a GitHub registry.
///
/// Requires the `github-registry` feature.
#[cfg(feature = "github-registry")]
pub struct GitHubRegistrySource {
    base_url: String,
    /// Calldata index: `"eip155:{chainId}:{address}"` → single relative path.
    calldata_index: HashMap<String, String>,
    /// EIP-712 index: `"eip155:{chainId}:{address}"` → `primaryType` buckets.
    eip712_index: HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>,
    /// In-memory descriptor cache keyed by relative path (tokio Mutex for async safety).
    cache: tokio::sync::Mutex<HashMap<String, Descriptor>>,
}

#[cfg(feature = "github-registry")]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Eip712IndexEntry {
    path: String,
    #[serde(rename = "encodeTypeHashes", default)]
    encode_type_hashes: Vec<String>,
}

#[cfg(feature = "github-registry")]
#[derive(Debug, serde::Deserialize)]
struct RegistryIndexV2 {
    calldata: HashMap<String, String>,
    eip712: HashMap<String, Vec<LegacyEip712IndexEntry>>,
}

#[cfg(feature = "github-registry")]
#[derive(Debug, Clone, serde::Deserialize)]
struct LegacyEip712IndexEntry {
    #[serde(rename = "primaryType")]
    primary_type: String,
    path: String,
}

#[cfg(feature = "github-registry")]
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

                // Resolve relative URL against the including file's directory
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
}

#[cfg(feature = "github-registry")]
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

#[cfg(feature = "github-registry")]
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

#[cfg(feature = "github-registry")]
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

#[cfg(feature = "github-registry")]
impl GitHubRegistrySource {
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

#[cfg(feature = "github-registry")]
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

pub(crate) struct TypedOuterSelection<'a> {
    pub matches: Vec<&'a ResolvedDescriptor>,
    pub domain_errors: Vec<String>,
    pub format_misses: Vec<&'a ResolvedDescriptor>,
}

pub(crate) fn select_matching_typed_descriptors<'a>(
    descriptors: &'a [ResolvedDescriptor],
    data: &crate::eip712::TypedData,
) -> Result<TypedOuterSelection<'a>, Error> {
    let Some(chain_id) = data.domain.chain_id else {
        return Ok(TypedOuterSelection {
            matches: Vec::new(),
            domain_errors: Vec::new(),
            format_misses: Vec::new(),
        });
    };
    let Some(verifying_contract) = data.domain.verifying_contract.as_deref() else {
        return Ok(TypedOuterSelection {
            matches: Vec::new(),
            domain_errors: Vec::new(),
            format_misses: Vec::new(),
        });
    };

    let mut matches = Vec::new();
    let mut domain_errors = Vec::new();
    let mut format_misses = Vec::new();

    for descriptor in descriptors {
        let deployment_matches = descriptor
            .descriptor
            .context
            .deployments()
            .iter()
            .any(|dep| {
                dep.chain_id == chain_id && dep.address.eq_ignore_ascii_case(verifying_contract)
            });
        if !deployment_matches {
            continue;
        }

        match crate::eip712::validate_descriptor_domain_binding(&descriptor.descriptor, data) {
            Ok(()) => {}
            Err(Error::Descriptor(message)) => {
                domain_errors.push(message);
                continue;
            }
            Err(err) => return Err(err),
        }

        match crate::eip712::find_typed_format_optional(&descriptor.descriptor, data)? {
            Some(_) => matches.push(descriptor),
            None => format_misses.push(descriptor),
        }
    }

    Ok(TypedOuterSelection {
        matches,
        domain_errors,
        format_misses,
    })
}

// ---------------------------------------------------------------------------
// Nested descriptor resolution
// ---------------------------------------------------------------------------

use crate::decoder::ArgumentValue;
use crate::types::display::{DisplayField, FieldFormat};

/// Maximum recursion depth for nested descriptor resolution.
const MAX_RESOLVE_DEPTH: u8 = 3;

/// Resolve all descriptors needed to format a transaction, including nested calldata.
///
/// 1. Resolves outer descriptor by `(chain_id, tx.to)` (or `implementation_address`)
/// 2. Finds `FieldFormat::Calldata` fields in the matching format
/// 3. Partially decodes outer calldata to extract inner callee addresses via `calleePath`
/// 4. Recursively resolves inner descriptors
/// 5. Returns `[outer, inner1, inner2, ...]` for use with `format_calldata`
///
/// If the outer descriptor is not found, returns an empty vec (graceful degradation).
/// Inner descriptor resolution failures are silently skipped — the engine will
/// produce a raw preview for those inner calls.
pub async fn resolve_descriptors_for_tx(
    tx: &crate::TransactionContext<'_>,
    source: &dyn DescriptorSource,
) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
    let mut results = Vec::new();
    let address = tx.implementation_address.unwrap_or(tx.to);
    resolve_recursive(
        tx.chain_id,
        address,
        tx.calldata,
        source,
        MAX_RESOLVE_DEPTH,
        &mut results,
    )
    .await?;
    Ok(results)
}

fn resolve_recursive<'a>(
    chain_id: u64,
    address: &'a str,
    calldata: &'a [u8],
    source: &'a dyn DescriptorSource,
    depth: u8,
    results: &'a mut Vec<ResolvedDescriptor>,
) -> Pin<Box<dyn Future<Output = Result<(), ResolveError>> + Send + 'a>> {
    Box::pin(async move {
        if depth == 0 || calldata.len() < 4 {
            return Ok(());
        }

        let resolved = match source.resolve_calldata(chain_id, address).await {
            Ok(r) => r,
            Err(ResolveError::NotFound { .. }) => return Ok(()),
            Err(e) => return Err(e),
        };

        // Find matching format key for this calldata's selector
        let selector = &calldata[..4];
        let (sig, format_key) = match crate::find_matching_signature(&resolved.descriptor, selector)
        {
            Ok(r) => r,
            Err(_) => {
                results.push(resolved);
                return Ok(());
            }
        };

        // Decode calldata to get arguments
        let decoded = match crate::decoder::decode_calldata(&sig, calldata) {
            Ok(d) => d,
            Err(_) => {
                results.push(resolved);
                return Ok(());
            }
        };

        // Find display format to walk fields
        let format = resolved.descriptor.display.formats.get(&format_key);

        // Collect calldata field info before pushing (avoids borrow issues with clone)
        let calldata_fields = format
            .map(|fmt| {
                collect_calldata_fields(&fmt.fields, &resolved.descriptor.display.definitions)
            })
            .unwrap_or_default();

        results.push(resolved);

        for field in &calldata_fields {
            let data_path = match &field.data_path {
                Some(p) => p,
                None => continue,
            };

            let callee = resolve_resolver_nested_callee(field, &decoded)
                .map_err(|e| ResolveError::Parse(e.to_string()))?;

            // Resolve inner calldata bytes
            let inner_data =
                crate::engine::resolve_path(&decoded, data_path).and_then(|v| match v {
                    ArgumentValue::Bytes(b) => Some(b),
                    _ => None,
                });

            let inner_chain = resolve_resolver_nested_chain_id(field, &decoded, chain_id)
                .map_err(|e| ResolveError::Parse(e.to_string()))?;
            let selector_override = resolve_resolver_nested_selector(field, &decoded)
                .map_err(|e| ResolveError::Parse(e.to_string()))?;

            if let (Some(addr), Some(data)) = (callee, inner_data) {
                let normalized =
                    crate::engine::normalized_nested_calldata(&data, selector_override);
                resolve_recursive(inner_chain, &addr, &normalized, source, depth - 1, results)
                    .await?;
            }
        }

        Ok(())
    })
}

/// Info extracted from a `FieldFormat::Calldata` display field.
pub(crate) struct CalldataFieldInfo {
    pub(crate) callee_path: Option<String>,
    pub(crate) callee: Option<String>,
    pub(crate) data_path: Option<String>,
    pub(crate) selector_path: Option<String>,
    pub(crate) selector: Option<String>,
    pub(crate) chain_id: Option<u64>,
    pub(crate) chain_id_path: Option<String>,
}

/// Walk display fields (resolving `$ref` references) and collect calldata-format fields.
pub(crate) fn collect_calldata_fields(
    fields: &[DisplayField],
    definitions: &HashMap<String, DisplayField>,
) -> Vec<CalldataFieldInfo> {
    let mut result = Vec::new();
    collect_calldata_fields_recursive(fields, definitions, &mut result);
    result
}

fn collect_calldata_fields_recursive(
    fields: &[DisplayField],
    definitions: &HashMap<String, DisplayField>,
    result: &mut Vec<CalldataFieldInfo>,
) {
    for field in fields {
        match field {
            DisplayField::Simple {
                path,
                format,
                params,
                ..
            } => {
                if matches!(format.as_ref(), Some(FieldFormat::Calldata)) {
                    let fp = params.as_ref();
                    result.push(CalldataFieldInfo {
                        callee_path: fp.and_then(|p| p.callee_path.clone()),
                        callee: fp.and_then(|p| p.callee.clone()),
                        data_path: path.clone(),
                        selector_path: fp.and_then(|p| p.selector_path.clone()),
                        selector: fp.and_then(|p| p.selector.clone()),
                        chain_id: fp.and_then(|p| p.chain_id),
                        chain_id_path: fp.and_then(|p| p.chain_id_path.clone()),
                    });
                }
            }
            DisplayField::Reference {
                reference,
                path,
                params: ref_params,
                ..
            } => {
                // Resolve the $ref to get the definition's format and params
                let def_key = reference
                    .strip_prefix("$.display.definitions.")
                    .unwrap_or(reference);
                if let Some(DisplayField::Simple {
                    format: def_format,
                    params: def_params,
                    ..
                }) = definitions.get(def_key)
                {
                    if matches!(def_format.as_ref(), Some(FieldFormat::Calldata)) {
                        // Merge params: ref_params override def_params
                        let callee_path = ref_params
                            .as_ref()
                            .and_then(|p| p.callee_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.callee_path.clone()));
                        let callee = ref_params
                            .as_ref()
                            .and_then(|p| p.callee.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.callee.clone()));
                        let selector_path = ref_params
                            .as_ref()
                            .and_then(|p| p.selector_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.selector_path.clone()));
                        let selector = ref_params
                            .as_ref()
                            .and_then(|p| p.selector.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.selector.clone()));
                        let chain_id = ref_params
                            .as_ref()
                            .and_then(|p| p.chain_id)
                            .or_else(|| def_params.as_ref().and_then(|p| p.chain_id));
                        let chain_id_path = ref_params
                            .as_ref()
                            .and_then(|p| p.chain_id_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.chain_id_path.clone()));
                        result.push(CalldataFieldInfo {
                            callee_path,
                            callee,
                            data_path: path.clone(),
                            selector_path,
                            selector,
                            chain_id,
                            chain_id_path,
                        });
                    }
                }
            }
            DisplayField::Group { field_group } => {
                collect_calldata_fields_recursive(&field_group.fields, definitions, result);
            }
            DisplayField::Scope { fields: sub, .. } => {
                collect_calldata_fields_recursive(sub, definitions, result);
            }
        }
    }
}

fn resolve_resolver_nested_callee(
    field: &CalldataFieldInfo,
    decoded: &crate::decoder::DecodedArguments,
) -> Result<Option<String>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.callee.is_some(),
        field.callee_path.is_some(),
        "callee",
    )?;
    if let Some(callee) = field.callee.as_deref() {
        return crate::engine::parse_nested_address_param(callee, "callee").map(Some);
    }
    Ok(field
        .callee_path
        .as_ref()
        .and_then(|p| crate::engine::resolve_path(decoded, p))
        .and_then(|value| crate::engine::address_string_from_argument_value(&value)))
}

fn resolve_resolver_nested_chain_id(
    field: &CalldataFieldInfo,
    decoded: &crate::decoder::DecodedArguments,
    default_chain_id: u64,
) -> Result<u64, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.chain_id.is_some(),
        field.chain_id_path.is_some(),
        "chainId",
    )?;
    if let Some(chain_id) = field.chain_id {
        return Ok(chain_id);
    }
    Ok(field
        .chain_id_path
        .as_ref()
        .and_then(|p| crate::engine::resolve_path(decoded, p))
        .and_then(|value| crate::engine::chain_id_from_argument_value(&value))
        .unwrap_or(default_chain_id))
}

fn resolve_resolver_nested_selector(
    field: &CalldataFieldInfo,
    decoded: &crate::decoder::DecodedArguments,
) -> Result<Option<[u8; 4]>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.selector.is_some(),
        field.selector_path.is_some(),
        "selector",
    )?;
    if let Some(selector) = field.selector.as_deref() {
        return crate::engine::parse_nested_selector_param(selector, "selector").map(Some);
    }
    Ok(field
        .selector_path
        .as_ref()
        .and_then(|p| crate::engine::resolve_path(decoded, p))
        .and_then(|value| crate::engine::selector_from_argument_value(&value)))
}

fn resolve_typed_nested_callee_for_resolver(
    field: &CalldataFieldInfo,
    message: &serde_json::Value,
    chain_id: u64,
    verifying_contract: &str,
) -> Result<Option<String>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.callee.is_some(),
        field.callee_path.is_some(),
        "callee",
    )?;
    if let Some(callee) = field.callee.as_deref() {
        return crate::engine::parse_nested_address_param(callee, "callee").map(Some);
    }
    Ok(field
        .callee_path
        .as_ref()
        .and_then(|p| {
            crate::eip712::resolve_typed_path(message, p, chain_id, Some(verifying_contract))
        })
        .as_ref()
        .and_then(crate::eip712::coerce_typed_address_string))
}

fn resolve_typed_nested_chain_id_for_resolver(
    field: &CalldataFieldInfo,
    message: &serde_json::Value,
    chain_id: u64,
    verifying_contract: &str,
) -> Result<u64, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.chain_id.is_some(),
        field.chain_id_path.is_some(),
        "chainId",
    )?;
    if let Some(chain_id) = field.chain_id {
        return Ok(chain_id);
    }
    Ok(field
        .chain_id_path
        .as_ref()
        .and_then(|p| {
            crate::eip712::resolve_typed_path(message, p, chain_id, Some(verifying_contract))
        })
        .as_ref()
        .and_then(crate::eip712::coerce_typed_numeric_string)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(chain_id))
}

fn resolve_typed_nested_selector_for_resolver(
    field: &CalldataFieldInfo,
    message: &serde_json::Value,
    chain_id: u64,
    verifying_contract: &str,
) -> Result<Option<[u8; 4]>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.selector.is_some(),
        field.selector_path.is_some(),
        "selector",
    )?;
    if let Some(selector) = field.selector.as_deref() {
        return crate::engine::parse_nested_selector_param(selector, "selector").map(Some);
    }
    Ok(field
        .selector_path
        .as_ref()
        .and_then(|p| {
            crate::eip712::resolve_typed_path(message, p, chain_id, Some(verifying_contract))
        })
        .as_ref()
        .and_then(crate::eip712::selector_from_typed_value))
}

/// Resolve all descriptors needed to format EIP-712 typed data, including nested calldata.
///
/// Uses the full typed-data schema so outer descriptor selection follows the same
/// `domain` / `domainSeparator` / exact `encodeType` rules as rendering.
pub async fn resolve_descriptors_for_typed_data(
    typed_data: &crate::eip712::TypedData,
    source: &dyn DescriptorSource,
) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
    let mut results = Vec::new();

    let Some(chain_id) = typed_data.domain.chain_id else {
        return Ok(results);
    };
    let Some(verifying_contract) = typed_data.domain.verifying_contract.as_deref() else {
        return Ok(results);
    };

    let lookup = TypedDescriptorLookup {
        chain_id,
        verifying_contract: verifying_contract.to_string(),
        primary_type: typed_data.primary_type.clone(),
        encode_type_hash: Some(
            crate::eip712::encode_type_hash_hex_for_primary_type(typed_data)
                .map_err(|e| ResolveError::Parse(e.to_string()))?,
        ),
    };

    let candidates = match source.resolve_typed_candidates(lookup).await {
        Ok(r) => r,
        Err(ResolveError::NotFound { .. }) => return Ok(results),
        Err(e) => return Err(e),
    };

    let selection = select_matching_typed_descriptors(&candidates, typed_data)
        .map_err(|e| ResolveError::Parse(e.to_string()))?;
    let outer = match selection.matches.len() {
        1 => selection.matches[0].clone(),
        0 => return Ok(results),
        _ => {
            return Err(ResolveError::Parse(format!(
                "multiple EIP-712 descriptors match chain_id={} verifying_contract={} after domain and encodeType validation",
                chain_id, verifying_contract
            )))
        }
    };

    let format = crate::eip712::find_typed_format(&outer.descriptor, typed_data)
        .map_err(|e| ResolveError::Parse(e.to_string()))?;
    let mut warnings = Vec::new();
    let expanded =
        crate::engine::expand_display_fields(&outer.descriptor, &format.fields, &mut warnings);
    let calldata_fields = collect_calldata_fields(&expanded, &outer.descriptor.display.definitions);

    results.push(outer);

    for field in &calldata_fields {
        let callee_addr = match resolve_typed_nested_callee_for_resolver(
            field,
            &typed_data.message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?
        {
            Some(addr) => addr,
            None => continue,
        };

        let inner_chain = resolve_typed_nested_chain_id_for_resolver(
            field,
            &typed_data.message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?;
        let selector_override = resolve_typed_nested_selector_for_resolver(
            field,
            &typed_data.message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?;

        if let Some(data_path) = &field.data_path {
            if let Some(inner_hex) = crate::eip712::resolve_typed_path(
                &typed_data.message,
                data_path,
                chain_id,
                Some(verifying_contract),
            )
            .and_then(|v| v.as_str().map(String::from))
            {
                let hex_str = inner_hex
                    .strip_prefix("0x")
                    .or_else(|| inner_hex.strip_prefix("0X"))
                    .unwrap_or(&inner_hex);
                if let Ok(inner_bytes) = hex::decode(hex_str) {
                    let normalized =
                        crate::engine::normalized_nested_calldata(&inner_bytes, selector_override);
                    let _ = resolve_recursive(
                        inner_chain,
                        &callee_addr,
                        &normalized,
                        source,
                        MAX_RESOLVE_DEPTH - 1,
                        &mut results,
                    )
                    .await;
                    continue;
                }
            }
        }

        match source.resolve_calldata(inner_chain, &callee_addr).await {
            Ok(r) => {
                if !results
                    .iter()
                    .any(|e| e.chain_id == r.chain_id && e.address.eq_ignore_ascii_case(&r.address))
                {
                    results.push(r);
                }
            }
            Err(ResolveError::NotFound { .. }) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eip712::TypedData;
    use crate::EmptyDataProvider;

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

    #[cfg(feature = "github-registry")]
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

    #[cfg(feature = "github-registry")]
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

    #[cfg(feature = "github-registry")]
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

    fn safe_descriptor() -> Descriptor {
        let json = std::fs::read_to_string(format!(
            "{}/tests/fixtures/common-Safe.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("read Safe descriptor");
        Descriptor::from_json(&json).expect("parse Safe descriptor")
    }

    fn erc20_descriptor() -> Descriptor {
        let json = std::fs::read_to_string(format!(
            "{}/tests/fixtures/erc20-transfer.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("read ERC-20 descriptor");
        Descriptor::from_json(&json).expect("parse ERC-20 descriptor")
    }

    fn address_word(hex_addr: &str) -> Vec<u8> {
        let hex_str = hex_addr
            .strip_prefix("0x")
            .or_else(|| hex_addr.strip_prefix("0X"))
            .unwrap_or(hex_addr);
        let addr_bytes = hex::decode(hex_str).expect("valid hex address");
        let mut word = vec![0u8; 12];
        word.extend_from_slice(&addr_bytes);
        assert_eq!(word.len(), 32);
        word
    }

    fn uint_word(val: u128) -> Vec<u8> {
        let mut word = vec![0u8; 16];
        word.extend_from_slice(&val.to_be_bytes());
        assert_eq!(word.len(), 32);
        word
    }

    fn pad32(len: usize) -> usize {
        len.div_ceil(32) * 32
    }

    /// Build ABI-encoded `execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)`
    fn build_exec_transaction_calldata(to: &str, inner_calldata: &[u8]) -> Vec<u8> {
        let sig = crate::decoder::parse_signature(
            "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)",
        )
        .unwrap();

        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        calldata.extend_from_slice(&address_word(to));
        calldata.extend_from_slice(&uint_word(0));
        calldata.extend_from_slice(&uint_word(320)); // data offset
        calldata.extend_from_slice(&uint_word(0)); // operation
        calldata.extend_from_slice(&uint_word(0)); // safeTxGas
        calldata.extend_from_slice(&uint_word(0)); // baseGas
        calldata.extend_from_slice(&uint_word(0)); // gasPrice
        calldata.extend_from_slice(&[0u8; 32]); // gasToken
        calldata.extend_from_slice(&[0u8; 32]); // refundReceiver
        let data_offset = 320 + 32 + pad32(inner_calldata.len());
        calldata.extend_from_slice(&uint_word(data_offset as u128)); // signatures offset

        // Data bytes
        calldata.extend_from_slice(&uint_word(inner_calldata.len() as u128));
        calldata.extend_from_slice(inner_calldata);
        let padding = pad32(inner_calldata.len()) - inner_calldata.len();
        calldata.extend_from_slice(&vec![0u8; padding]);

        // Empty signatures
        calldata.extend_from_slice(&uint_word(0));

        calldata
    }

    fn build_erc20_transfer_calldata(to: &str, amount: u128) -> Vec<u8> {
        let sig = crate::decoder::parse_signature("transfer(address,uint256)").unwrap();
        let mut calldata = Vec::new();
        calldata.extend_from_slice(&sig.selector);
        calldata.extend_from_slice(&address_word(to));
        calldata.extend_from_slice(&uint_word(amount));
        calldata
    }

    fn permit2_descriptor(
        owner: &str,
        format_key: &str,
        extra_context: Option<serde_json::Value>,
    ) -> Descriptor {
        let mut eip712 = serde_json::json!({
            "deployments": [{
                "chainId": 1,
                "address": "0x000000000022d473030f116ddee9f6b43ac78ba3"
            }],
            "domain": {
                "name": "Permit2"
            }
        });

        if let Some(extra) = extra_context {
            let target = eip712.as_object_mut().expect("eip712 context object");
            for (key, value) in extra.as_object().expect("extra context object") {
                target.insert(key.clone(), value.clone());
            }
        }

        let descriptor = serde_json::json!({
            "context": { "eip712": eip712 },
            "metadata": {
                "owner": owner,
                "enums": {},
                "constants": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {
                    format_key: {
                        "intent": owner,
                        "fields": [{
                            "path": "spender",
                            "label": "Spender",
                            "format": "raw"
                        }]
                    }
                }
            }
        });
        Descriptor::from_json(&descriptor.to_string()).expect("descriptor")
    }

    fn exclusive_dutch_order_typed_data() -> TypedData {
        serde_json::from_value(serde_json::json!({
            "types": {
                "PermitWitnessTransferFrom": [
                    { "name": "permitted", "type": "TokenPermissions" },
                    { "name": "spender", "type": "address" },
                    { "name": "nonce", "type": "uint256" },
                    { "name": "deadline", "type": "uint256" },
                    { "name": "witness", "type": "ExclusiveDutchOrder" }
                ],
                "TokenPermissions": [
                    { "name": "token", "type": "address" },
                    { "name": "amount", "type": "uint256" }
                ],
                "ExclusiveDutchOrder": [
                    { "name": "info", "type": "OrderInfo" },
                    { "name": "decayStartTime", "type": "uint256" },
                    { "name": "decayEndTime", "type": "uint256" },
                    { "name": "exclusiveFiller", "type": "address" },
                    { "name": "exclusivityOverrideBps", "type": "uint256" },
                    { "name": "inputToken", "type": "address" },
                    { "name": "inputStartAmount", "type": "uint256" },
                    { "name": "inputEndAmount", "type": "uint256" },
                    { "name": "outputs", "type": "DutchOutput[]" }
                ],
                "OrderInfo": [
                    { "name": "reactor", "type": "address" },
                    { "name": "swapper", "type": "address" },
                    { "name": "nonce", "type": "uint256" },
                    { "name": "deadline", "type": "uint256" },
                    { "name": "additionalValidationContract", "type": "address" },
                    { "name": "additionalValidationData", "type": "bytes" }
                ],
                "DutchOutput": [
                    { "name": "token", "type": "address" },
                    { "name": "startAmount", "type": "uint256" },
                    { "name": "endAmount", "type": "uint256" },
                    { "name": "recipient", "type": "address" }
                ],
                "EIP712Domain": [
                    { "name": "name", "type": "string" },
                    { "name": "chainId", "type": "uint256" },
                    { "name": "verifyingContract", "type": "address" }
                ]
            },
            "domain": {
                "name": "Permit2",
                "chainId": "1",
                "verifyingContract": "0x000000000022d473030f116ddee9f6b43ac78ba3"
            },
            "primaryType": "PermitWitnessTransferFrom",
            "message": {
                "permitted": {
                    "token": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                    "amount": "100000000000000"
                },
                "spender": "0x6000da47483062a0d734ba3dc7576ce6a0b645c4",
                "nonce": "1993349843209468715141873868895370562722298771555073489698616037339384894721",
                "deadline": "1774866877",
                "witness": {
                    "info": {
                        "reactor": "0x6000da47483062a0d734ba3dc7576ce6a0b645c4",
                        "swapper": "0xbf01daf454dce008d3e2bfd47d5e186f71477253",
                        "nonce": "1993349843209468715141873868895370562722298771555073489698616037339384894721",
                        "deadline": "1774866877",
                        "additionalValidationContract": "0x0000000000000000000000000000000000000000",
                        "additionalValidationData": "0x"
                    },
                    "decayStartTime": "1774780477",
                    "decayEndTime": "1774780477",
                    "exclusiveFiller": "0x0000000000000000000000000000000000000000",
                    "exclusivityOverrideBps": "0",
                    "inputToken": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                    "inputStartAmount": "100000000000000",
                    "inputEndAmount": "100000000000000",
                    "outputs": [{
                        "token": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                        "startAmount": "199179",
                        "endAmount": "199179",
                        "recipient": "0xbf01daf454dce008d3e2bfd47d5e186f71477253"
                    }]
                }
            }
        }))
        .expect("typed data")
    }

    #[tokio::test]
    async fn test_resolve_descriptors_safe_wrapping_erc20() {
        let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
        let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
        let recipient = "0x1234567890123456789012345678901234567890";

        let mut source = StaticSource::new();
        source.add_calldata(1, safe_addr, safe_descriptor());
        source.add_calldata(1, usdc_addr, erc20_descriptor());

        let inner = build_erc20_transfer_calldata(recipient, 1_000_000);
        let outer = build_exec_transaction_calldata(usdc_addr, &inner);

        let tx = crate::TransactionContext {
            chain_id: 1,
            to: safe_addr,
            calldata: &outer,
            value: None,
            from: None,
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();

        assert_eq!(descriptors.len(), 2, "should resolve outer + inner");
        assert_eq!(descriptors[0].address, safe_addr.to_lowercase());
        assert_eq!(descriptors[1].address, usdc_addr.to_lowercase());
    }

    #[tokio::test]
    async fn test_resolve_descriptors_unknown_inner() {
        let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
        let unknown_addr = "0x0000000000000000000000000000000000000042";

        let mut source = StaticSource::new();
        source.add_calldata(1, safe_addr, safe_descriptor());
        // No descriptor for unknown_addr

        let inner =
            hex::decode("12345678000000000000000000000000000000000000000000000000000000000000002a")
                .unwrap();
        let outer = build_exec_transaction_calldata(unknown_addr, &inner);

        let tx = crate::TransactionContext {
            chain_id: 1,
            to: safe_addr,
            calldata: &outer,
            value: None,
            from: None,
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();

        assert_eq!(
            descriptors.len(),
            1,
            "should resolve outer only, gracefully skip unknown inner"
        );
        assert_eq!(descriptors[0].address, safe_addr.to_lowercase());
    }

    #[tokio::test]
    async fn test_resolve_descriptors_no_outer() {
        let source = StaticSource::new();

        let calldata = hex::decode("a9059cbb").unwrap();
        let tx = crate::TransactionContext {
            chain_id: 1,
            to: "0x0000000000000000000000000000000000000099",
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();
        assert!(descriptors.is_empty(), "no outer descriptor → empty vec");
    }

    #[tokio::test]
    async fn test_resolve_descriptors_uses_implementation_address() {
        let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
        let proxy_addr = "0x1111111111111111111111111111111111111111";
        let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

        let mut source = StaticSource::new();
        // Descriptor registered under implementation address, not proxy
        source.add_calldata(1, safe_addr, safe_descriptor());
        source.add_calldata(1, usdc_addr, erc20_descriptor());

        let inner =
            build_erc20_transfer_calldata("0x1234567890123456789012345678901234567890", 1_000_000);
        let outer = build_exec_transaction_calldata(usdc_addr, &inner);

        let tx = crate::TransactionContext {
            chain_id: 1,
            to: proxy_addr,
            calldata: &outer,
            value: None,
            from: None,
            implementation_address: Some(safe_addr),
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();

        assert_eq!(
            descriptors.len(),
            2,
            "should use implementation_address for outer resolution"
        );
    }

    #[tokio::test]
    async fn test_resolve_typed_data_selects_exact_encode_type_candidate() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Dutch Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,DutchOrder witness)DutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Exclusive Dutch Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Limit Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,LimitOrder witness)LimitOrder(OrderInfo info,address inputToken,uint256 inputAmount,OutputToken[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)OutputToken(address token,uint256 amount,address recipient)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );

        let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .expect("resolve");
        assert_eq!(descriptors.len(), 1);
        assert_eq!(
            descriptors[0].descriptor.metadata.owner.as_deref(),
            Some("Exclusive Dutch Order")
        );

        let model = crate::format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
            .await
            .expect("format");
        assert_eq!(model.intent, "Exclusive Dutch Order");
    }

    #[tokio::test]
    async fn test_resolve_typed_data_returns_empty_when_no_exact_encode_type_candidate_matches() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Dutch Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,DutchOrder witness)DutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );

        let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .expect("resolve");
        assert!(descriptors.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_typed_data_rejects_domain_separator_mismatch_before_exact_match() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Wrong Separator",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                Some(serde_json::json!({
                    "domainSeparator": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                })),
            ),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Correct Candidate",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );

        let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .expect("resolve");
        assert_eq!(descriptors.len(), 1);
        assert_eq!(
            descriptors[0].descriptor.metadata.owner.as_deref(),
            Some("Correct Candidate")
        );
    }

    #[tokio::test]
    async fn test_resolve_typed_data_errors_when_multiple_exact_candidates_survive() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        let format_key = "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)";
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor("Candidate A", format_key, None),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor("Candidate B", format_key, None),
        );

        let err = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("multiple EIP-712 descriptors match"));
    }
}
