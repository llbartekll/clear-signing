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
    /// Maps "{chain_id}:{address_lowercase}" → list of relative paths in registry.
    /// Multiple paths per key when both calldata and EIP-712 descriptors exist.
    index: HashMap<String, Vec<String>>,
    /// In-memory descriptor cache keyed by relative path (tokio Mutex for async safety)
    cache: tokio::sync::Mutex<HashMap<String, Descriptor>>,
}

#[cfg(feature = "github-registry")]
impl GitHubRegistrySource {
    /// Create a new source with a manually provided index (single path per key).
    ///
    /// `base_url`: raw content URL prefix (e.g., `"https://raw.githubusercontent.com/org/repo/main"`).
    /// `index`: maps `"{chain_id}:{address}"` → relative path (e.g., `"aave/calldata-lpv3.json"`).
    pub fn new(base_url: &str, index: HashMap<String, String>) -> Self {
        let multi: HashMap<String, Vec<String>> =
            index.into_iter().map(|(k, v)| (k, vec![v])).collect();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            index: multi,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Create a source by fetching `index.json` from the registry.
    ///
    /// Index values can be a single string or an array of strings (when both
    /// calldata and EIP-712 descriptors exist for the same address).
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
        let raw: HashMap<String, serde_json::Value> =
            serde_json::from_str(&body).map_err(|e| ResolveError::Parse(e.to_string()))?;

        // Normalize: string → vec![string], array → vec of strings
        let index: HashMap<String, Vec<String>> = raw
            .into_iter()
            .filter_map(|(k, v)| match v {
                serde_json::Value::String(s) => Some((k, vec![s])),
                serde_json::Value::Array(arr) => {
                    let paths: Vec<String> = arr
                        .into_iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if paths.is_empty() {
                        None
                    } else {
                        Some((k, paths))
                    }
                }
                _ => None,
            })
            .collect();

        let eip712_count = index.values().filter(|paths: &&Vec<String>| paths.iter().any(|p| p.contains("eip712"))).count();
        println!("[erc7730] from_registry: loaded {} index entries ({} with eip712 paths) from {}", index.len(), eip712_count, index_url);

        Ok(Self {
            base_url: base.to_string(),
            index,
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
        println!("[erc7730] fetch_descriptor: {}", rel_path);
        let value = self
            .fetch_and_merge_value(rel_path, Self::MAX_INCLUDES_DEPTH)
            .await?;
        let result = serde_json::from_value::<Descriptor>(value);
        match &result {
            Ok(desc) => {
                let ctx_type = match &desc.context {
                    crate::types::context::DescriptorContext::Eip712(_) => "eip712",
                    crate::types::context::DescriptorContext::Contract(_) => "contract",
                };
                let format_keys: Vec<&String> = desc.display.formats.keys().collect();
                println!("[erc7730] fetch_descriptor: OK context={}, format_keys={:?}", ctx_type, format_keys);
            }
            Err(e) => println!("[erc7730] fetch_descriptor: PARSE ERROR: {}", e),
        }
        result.map_err(|e| ResolveError::Parse(e.to_string()))
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
                let included_value =
                    self.fetch_and_merge_value(&resolved_path, depth - 1).await?;

                Ok(crate::merge::merge_descriptor_values(&value, &included_value))
            } else {
                Ok(value)
            }
        })
    }
}

#[cfg(feature = "github-registry")]
impl GitHubRegistrySource {
    /// Resolve a descriptor for a given key, preferring the given context type.
    ///
    /// Tries each path for the key. If a descriptor with the preferred context
    /// type is found, returns it immediately. Otherwise falls back to the first
    /// successfully fetched descriptor (backwards compatible for single-entry keys).
    async fn resolve_by_context(
        &self,
        chain_id: u64,
        address: &str,
        prefer_eip712: bool,
        primary_type: Option<&str>,
    ) -> Result<ResolvedDescriptor, ResolveError> {
        let address_owned = address.to_lowercase();
        let key = Self::make_key(chain_id, &address_owned);
        println!("[erc7730] resolve_by_context: key={}, prefer_eip712={}, primary_type={:?}", key, prefer_eip712, primary_type);

        let paths = self
            .index
            .get(&key)
            .ok_or_else(|| {
                println!("[erc7730] resolve_by_context: key NOT FOUND in index");
                ResolveError::NotFound {
                    chain_id,
                    address: address_owned.clone(),
                }
            })?
            .clone();
        println!("[erc7730] resolve_by_context: found {} paths: {:?}", paths.len(), paths);

        // Helper: check if a descriptor's format keys match the primary_type filter.
        let format_keys_match = |desc: &Descriptor, pt: &str| -> bool {
            let prefix = format!("{}(", pt);
            desc.display
                .formats
                .keys()
                .any(|k| k == pt || k.starts_with(&prefix))
        };

        // Check cache for a descriptor with matching context (+ primary_type when set)
        {
            let cache = self.cache.lock().await;
            let mut context_fallback = None;
            let mut any_fallback = None;
            for path in &paths {
                if let Some(cached) = cache.get(path) {
                    let context_matches = match &cached.context {
                        crate::types::context::DescriptorContext::Eip712(_) => prefer_eip712,
                        crate::types::context::DescriptorContext::Contract(_) => !prefer_eip712,
                    };
                    if context_matches {
                        if let Some(pt) = primary_type {
                            if format_keys_match(cached, pt) {
                                println!("[erc7730] resolve_by_context: cache HIT with primary_type match: {}", path);
                                return Ok(ResolvedDescriptor {
                                    descriptor: cached.clone(),
                                    chain_id,
                                    address: address_owned,
                                });
                            }
                            if context_fallback.is_none() {
                                context_fallback = Some(cached.clone());
                            }
                        } else {
                            return Ok(ResolvedDescriptor {
                                descriptor: cached.clone(),
                                chain_id,
                                address: address_owned,
                            });
                        }
                    }
                    if any_fallback.is_none() {
                        any_fallback = Some(cached.clone());
                    }
                }
            }
            // If all paths are cached but none matched primary_type, use context fallback
            if paths.iter().all(|p| cache.contains_key(p.as_str())) {
                if let Some(desc) = context_fallback.or(any_fallback) {
                    println!("[erc7730] resolve_by_context: cache fallback (no primary_type match)");
                    return Ok(ResolvedDescriptor {
                        descriptor: desc,
                        chain_id,
                        address: address_owned,
                    });
                }
            }
        }

        // Fetch uncached descriptors, return first with matching context + primary_type
        let mut context_fallback = None;
        let mut any_fallback = None;
        for path in &paths {
            // Skip if already cached and didn't match above
            if self.cache.lock().await.contains_key(path.as_str()) {
                continue;
            }

            let descriptor = match self.fetch_descriptor(path).await {
                Ok(d) => d,
                Err(_) => continue,
            };
            self.cache
                .lock()
                .await
                .insert(path.clone(), descriptor.clone());

            let context_matches = match &descriptor.context {
                crate::types::context::DescriptorContext::Eip712(_) => prefer_eip712,
                crate::types::context::DescriptorContext::Contract(_) => !prefer_eip712,
            };
            if context_matches {
                if let Some(pt) = primary_type {
                    if format_keys_match(&descriptor, pt) {
                        println!("[erc7730] resolve_by_context: fetched with primary_type match: {}", path);
                        return Ok(ResolvedDescriptor {
                            descriptor,
                            chain_id,
                            address: address_owned,
                        });
                    }
                    if context_fallback.is_none() {
                        context_fallback = Some(descriptor);
                    }
                } else {
                    return Ok(ResolvedDescriptor {
                        descriptor,
                        chain_id,
                        address: address_owned,
                    });
                }
            } else if any_fallback.is_none() {
                any_fallback = Some(descriptor);
            }
        }

        // Fallback: prefer context match, then any descriptor
        match context_fallback.or(any_fallback) {
            Some(desc) => {
                println!("[erc7730] resolve_by_context: fallback (no primary_type match found)");
                Ok(ResolvedDescriptor {
                    descriptor: desc,
                    chain_id,
                    address: address_owned,
                })
            }
            None => Err(ResolveError::NotFound {
                chain_id,
                address: address_owned,
            }),
        }
    }

    /// Resolve an EIP-712 descriptor filtered by `primary_type`.
    ///
    /// Prefers a descriptor whose format keys match `primary_type` (exact or
    /// prefix like `"SafeTx(..."`) over other eip712 descriptors for the same address.
    pub async fn resolve_typed_for_primary_type(
        &self,
        chain_id: u64,
        address: &str,
        primary_type: &str,
    ) -> Result<ResolvedDescriptor, ResolveError> {
        self.resolve_by_context(chain_id, address, true, Some(primary_type))
            .await
    }
}

#[cfg(feature = "github-registry")]
impl DescriptorSource for GitHubRegistrySource {
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let addr = address.to_string();
        Box::pin(async move { self.resolve_by_context(chain_id, &addr, false, None).await })
    }

    fn resolve_typed(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let addr = address.to_string();
        Box::pin(async move { self.resolve_by_context(chain_id, &addr, true, None).await })
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
            let callee_path = match &field.callee_path {
                Some(p) => p,
                None => continue,
            };
            let data_path = match &field.data_path {
                Some(p) => p,
                None => continue,
            };

            // Resolve callee address from decoded arguments
            let callee = crate::engine::resolve_path(&decoded, callee_path).and_then(|v| match v {
                ArgumentValue::Address(addr) => Some(format!("0x{}", hex::encode(addr))),
                _ => None,
            });

            // Resolve inner calldata bytes
            let inner_data =
                crate::engine::resolve_path(&decoded, data_path).and_then(|v| match v {
                    ArgumentValue::Bytes(b) => Some(b),
                    _ => None,
                });

            // Resolve inner chain_id (from chainIdPath, or default to outer)
            let inner_chain = field
                .chain_id_path
                .as_ref()
                .and_then(|p| crate::engine::resolve_path(&decoded, p))
                .and_then(|v| match v {
                    ArgumentValue::Uint(bytes) => {
                        let n = num_bigint::BigUint::from_bytes_be(&bytes);
                        u64::try_from(n).ok()
                    }
                    _ => None,
                })
                .unwrap_or(chain_id);

            if let (Some(addr), Some(data)) = (callee, inner_data) {
                resolve_recursive(inner_chain, &addr, &data, source, depth - 1, results).await?;
            }
        }

        Ok(())
    })
}

/// Info extracted from a `FieldFormat::Calldata` display field.
pub(crate) struct CalldataFieldInfo {
    pub(crate) callee_path: Option<String>,
    pub(crate) data_path: Option<String>,
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
                        data_path: path.clone(),
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
                        let chain_id_path = ref_params
                            .as_ref()
                            .and_then(|p| p.chain_id_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.chain_id_path.clone()));
                        result.push(CalldataFieldInfo {
                            callee_path,
                            data_path: path.clone(),
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

// ---------------------------------------------------------------------------
// Nested descriptor resolution for EIP-712 typed data
// ---------------------------------------------------------------------------

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
        let callee_path = match &field.callee_path {
            Some(p) => p,
            None => continue,
        };

        // Strip `#.` prefix (message-relative) same as render_typed_calldata_field does
        let callee_key = callee_path.strip_prefix("#.").unwrap_or(callee_path);
        let callee_addr = crate::eip712::resolve_typed_path(message, callee_key)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s),
                _ => None,
            });

        let callee_addr = match callee_addr {
            Some(addr) => addr,
            None => continue,
        };

        // Resolve inner chain_id (from chainIdPath in message, or default to outer)
        let inner_chain = field
            .chain_id_path
            .as_ref()
            .and_then(|p| {
                let path = p.strip_prefix("#.").unwrap_or(p);
                crate::eip712::resolve_typed_path(message, path)
            })
            .and_then(|v| match v {
                serde_json::Value::Number(n) => n.as_u64(),
                serde_json::Value::String(s) => s.parse::<u64>().ok(),
                _ => None,
            })
            .unwrap_or(chain_id);

        // Try to get inner calldata bytes for deeper nesting via resolve_recursive
        if let Some(data_path) = &field.data_path {
            let path = data_path.strip_prefix("#.").unwrap_or(data_path);
            if let Some(inner_hex) = crate::eip712::resolve_typed_path(message, path)
                .and_then(|v| v.as_str().map(String::from))
            {
                let hex_str = inner_hex
                    .strip_prefix("0x")
                    .or_else(|| inner_hex.strip_prefix("0X"))
                    .unwrap_or(&inner_hex);
                if let Ok(inner_bytes) = hex::decode(hex_str) {
                    // Use resolve_recursive for the inner calldata (reuses calldata flow)
                    let _ = resolve_recursive(
                        inner_chain,
                        &callee_addr,
                        &inner_bytes,
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

    println!(
        "[erc7730] resolve_descriptors_for_typed_data: resolved {} total descriptors",
        results.len()
    );
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
