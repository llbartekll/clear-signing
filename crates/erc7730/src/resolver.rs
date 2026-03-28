//! Pluggable descriptor resolution via the [`DescriptorSource`] trait.
//! Includes [`StaticSource`] for testing and embedded use cases.

use std::collections::HashMap;
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
    /// Calldata index: `"eip155:{chainId}:{address}"` → single relative path.
    calldata_index: HashMap<String, String>,
    /// EIP-712 index: `"eip155:{chainId}:{address}"` → list of (primaryType, path) entries.
    eip712_index: HashMap<String, Vec<Eip712IndexEntry>>,
    /// In-memory descriptor cache keyed by relative path (tokio Mutex for async safety).
    cache: tokio::sync::Mutex<HashMap<String, Descriptor>>,
}

#[cfg(feature = "github-registry")]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Eip712IndexEntry {
    #[serde(rename = "primaryType")]
    primary_type: String,
    path: String,
}

#[cfg(feature = "github-registry")]
#[derive(Debug, serde::Deserialize)]
struct RegistryIndex {
    calldata: HashMap<String, String>,
    eip712: HashMap<String, Vec<Eip712IndexEntry>>,
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
        eip712_index: HashMap<String, Vec<Eip712IndexEntry>>,
    ) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            calldata_index,
            eip712_index,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Create a source by fetching V2 `index.json` from the registry.
    ///
    /// The V2 index has two top-level keys:
    /// - `calldata`: `{key: "path"}` (string values, 1:1)
    /// - `eip712`: `{key: [{primaryType, path}]}` (array values with primaryType metadata)
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
        let index_v2: RegistryIndex =
            serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))?;

        Ok(Self {
            base_url: base.to_string(),
            calldata_index: index_v2.calldata,
            eip712_index: index_v2.eip712,
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
impl GitHubRegistrySource {
    /// Resolve an EIP-712 descriptor filtered by `primary_type`.
    ///
    /// Looks up the eip712 index by `(chain_id, address)`, finds the entry matching
    /// `primary_type`, and fetches that single descriptor.
    pub async fn resolve_typed_for_primary_type(
        &self,
        chain_id: u64,
        address: &str,
        primary_type: &str,
    ) -> Result<ResolvedDescriptor, ResolveError> {
        let address_lower = address.to_lowercase();
        let key = Self::make_key(chain_id, &address_lower);
        let entries = self
            .eip712_index
            .get(&key)
            .ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: address_lower.clone(),
            })?;
        let entry = entries
            .iter()
            .find(|e| e.primary_type == primary_type)
            .ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: address_lower.clone(),
            })?;
        let descriptor = self.fetch_descriptor_cached(&entry.path).await?;
        Ok(ResolvedDescriptor {
            descriptor,
            chain_id,
            address: address_lower,
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

    fn resolve_typed(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let addr = address.to_lowercase();
        Box::pin(async move {
            let key = Self::make_key(chain_id, &addr);
            let entries = self
                .eip712_index
                .get(&key)
                .ok_or_else(|| ResolveError::NotFound {
                    chain_id,
                    address: addr.clone(),
                })?;
            let entry = entries.first().ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: addr.clone(),
            })?;
            let descriptor = self.fetch_descriptor_cached(&entry.path).await?;
            Ok(ResolvedDescriptor {
                descriptor,
                chain_id,
                address: addr,
            })
        })
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

#[cfg(feature = "github-registry")]
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

#[cfg(feature = "github-registry")]
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

#[cfg(feature = "github-registry")]
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

// ---------------------------------------------------------------------------
// Nested descriptor resolution for EIP-712 typed data
// ---------------------------------------------------------------------------

/// Resolve all descriptors needed to format EIP-712 typed data using the full
/// typed-data schema, so nested calldata discovery follows the same format
/// selection rules as rendering.
#[cfg(feature = "github-registry")]
pub(crate) async fn resolve_descriptors_for_typed_data_with_types(
    typed_data: &crate::eip712::TypedData,
    source: &GitHubRegistrySource,
) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
    let mut results = Vec::new();

    let Some(chain_id) = typed_data.domain.chain_id else {
        return Ok(results);
    };
    let Some(verifying_contract) = typed_data.domain.verifying_contract.as_deref() else {
        return Ok(results);
    };

    let outer = match source
        .resolve_typed_for_primary_type(chain_id, verifying_contract, &typed_data.primary_type)
        .await
    {
        Ok(r) => r,
        Err(ResolveError::NotFound { .. }) => return Ok(results),
        Err(e) => return Err(e),
    };

    let format = crate::eip712::find_typed_format(&outer.descriptor, typed_data).ok();
    let calldata_fields = if let Some(format) = format {
        let mut warnings = Vec::new();
        let expanded =
            crate::engine::expand_display_fields(&outer.descriptor, &format.fields, &mut warnings);
        collect_calldata_fields(&expanded, &outer.descriptor.display.definitions)
    } else {
        Vec::new()
    };

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

/// Resolve all descriptors needed to format EIP-712 typed data, including nested calldata.
///
/// 1. Resolves outer EIP-712 descriptor by `(chain_id, verifying_contract, primary_type)`
/// 2. Finds `FieldFormat::Calldata` fields in the matching format
/// 3. Reads inner callee addresses from the EIP-712 JSON message via `calleePath`
/// 4. Resolves inner calldata descriptors from the registry
/// 5. Returns `[outer, inner1, inner2, ...]` for use with `format_typed_data`
///
/// If the outer descriptor is not found, returns an empty vec (graceful degradation).
/// Inner descriptor resolution failures are silently skipped.
#[cfg(feature = "github-registry")]
pub async fn resolve_descriptors_for_typed_data(
    chain_id: u64,
    verifying_contract: &str,
    primary_type: &str,
    message: &serde_json::Value,
    source: &GitHubRegistrySource,
) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
    let mut results = Vec::new();

    // 1. Resolve outer EIP-712 descriptor (with primary_type filter)
    let outer = match source
        .resolve_typed_for_primary_type(chain_id, verifying_contract, primary_type)
        .await
    {
        Ok(r) => r,
        Err(ResolveError::NotFound { .. }) => return Ok(results),
        Err(e) => return Err(e),
    };

    // 2. Find matching format key for this primary type
    let format = outer
        .descriptor
        .display
        .formats
        .get(primary_type)
        .or_else(|| {
            let prefix = format!("{}(", primary_type);
            outer
                .descriptor
                .display
                .formats
                .iter()
                .find(|(key, _)| key.starts_with(&prefix))
                .map(|(_, v)| v)
        });

    // 3. Collect calldata fields from the format
    let calldata_fields = format
        .map(|fmt| collect_calldata_fields(&fmt.fields, &outer.descriptor.display.definitions))
        .unwrap_or_default();

    results.push(outer);

    // 4. For each calldata field, resolve inner callee from JSON message
    for field in &calldata_fields {
        let callee_addr = match resolve_typed_nested_callee_for_resolver(
            field,
            message,
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
            message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?;
        let selector_override = resolve_typed_nested_selector_for_resolver(
            field,
            message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?;

        // Try to get inner calldata bytes for deeper nesting via resolve_recursive
        if let Some(data_path) = &field.data_path {
            if let Some(inner_hex) = crate::eip712::resolve_typed_path(
                message,
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
                    // Use resolve_recursive for the inner calldata (reuses calldata flow)
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

        // Fallback: resolve inner descriptor without deeper nesting
        match source.resolve_calldata(inner_chain, &callee_addr).await {
            Ok(inner_rd) => results.push(inner_rd),
            Err(_) => continue,
        }
    }

    Ok(results)
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
}
