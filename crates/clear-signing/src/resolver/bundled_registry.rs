//! Offline descriptor source backed by an embedded registry snapshot.
//!
//! The snapshot at `src/assets/registry-snapshot/` is vendored from the
//! upstream ERC-7730 registry via `cargo xtask update-registry-snapshot`
//! (never hand-edited) and embedded into the binary with `include_dir`.
//! Requires the `bundled-registry` feature.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use include_dir::{include_dir, Dir};

use crate::error::ResolveError;
use crate::types::descriptor::Descriptor;

use super::registry_common::{filter_typed_index_entries, resolve_relative_path, Eip712IndexEntry};
use super::source::{DescriptorSource, ResolvedDescriptor, TypedDescriptorLookup};

// NOTE: include_dir path lookups use `/`-separated relative paths, matching
// the index paths on unix hosts; Windows-host builds are untested.
static REGISTRY_SNAPSHOT: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/src/assets/registry-snapshot");

/// Descriptor source resolving from the registry snapshot embedded in the
/// binary. Fully offline: no network, no filesystem IO at resolve time.
pub struct BundledRegistrySource {
    /// Calldata index: `"eip155:{chainId}:{address}"` → single relative path.
    calldata_index: HashMap<String, String>,
    /// EIP-712 index: `"eip155:{chainId}:{address}"` → `primaryType` buckets.
    eip712_index: HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>,
}

impl BundledRegistrySource {
    /// Maximum depth for nested `includes` resolution.
    const MAX_INCLUDES_DEPTH: u8 = 3;

    /// Create a source by parsing the embedded split V3 index files.
    ///
    /// Errors indicate a broken snapshot (build-time data), so they are
    /// surfaced eagerly rather than deferred to resolve calls.
    pub fn new() -> Result<Self, ResolveError> {
        let calldata_index = parse_index::<HashMap<String, String>>("index.calldata.json")?;
        let eip712_index = parse_index::<HashMap<String, HashMap<String, Vec<Eip712IndexEntry>>>>(
            "index.eip712.json",
        )?;
        Ok(Self {
            calldata_index,
            eip712_index,
        })
    }

    fn make_key(chain_id: u64, address: &str) -> String {
        format!("eip155:{}:{}", chain_id, address.to_lowercase())
    }

    fn read_raw(rel_path: &str) -> Result<&'static str, ResolveError> {
        let file = REGISTRY_SNAPSHOT.get_file(rel_path).ok_or_else(|| {
            ResolveError::RegistryDescriptorMissing {
                url: format!("bundled:{rel_path}"),
            }
        })?;
        file.contents_utf8().ok_or_else(|| {
            ResolveError::RegistryIo(format!("bundled file is not valid UTF-8: {rel_path}"))
        })
    }

    fn load_descriptor(&self, rel_path: &str) -> Result<Descriptor, ResolveError> {
        let value = Self::load_and_merge_value(rel_path, Self::MAX_INCLUDES_DEPTH)?;
        serde_json::from_value::<Descriptor>(value).map_err(|e| ResolveError::Parse(e.to_string()))
    }

    /// Load a descriptor JSON and recursively resolve `includes`, returning
    /// the merged JSON value. Deserialization into [`Descriptor`] happens only
    /// at the top-level caller so that partial included files (which may lack
    /// required fields like `context`) don't cause parse errors.
    fn load_and_merge_value(rel_path: &str, depth: u8) -> Result<serde_json::Value, ResolveError> {
        let body = Self::read_raw(rel_path)?;
        let value: serde_json::Value =
            serde_json::from_str(body).map_err(|e| ResolveError::Parse(e.to_string()))?;

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
            let included_value = Self::load_and_merge_value(&resolved_path, depth - 1)?;

            Ok(crate::merge::merge_descriptor_values(
                &value,
                &included_value,
            ))
        } else {
            Ok(value)
        }
    }

    fn resolve_calldata_sync(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Result<ResolvedDescriptor, ResolveError> {
        let addr = address.to_lowercase();
        let key = Self::make_key(chain_id, &addr);
        let path = self
            .calldata_index
            .get(&key)
            .ok_or_else(|| ResolveError::NotFound {
                chain_id,
                address: addr.clone(),
            })?;
        let descriptor = self.load_descriptor(path)?;
        Ok(ResolvedDescriptor {
            descriptor,
            chain_id,
            address: addr,
        })
    }

    fn resolve_typed_candidates_sync(
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
                address: address_lower.clone(),
            });
        }

        let mut seen_paths = HashSet::new();
        let mut candidates = Vec::new();
        for entry in filtered_entries {
            if !seen_paths.insert(entry.path.as_str()) {
                continue;
            }
            let descriptor = self.load_descriptor(&entry.path)?;
            candidates.push(ResolvedDescriptor {
                descriptor,
                chain_id: lookup.chain_id,
                address: address_lower.clone(),
            });
        }
        Ok(candidates)
    }
}

impl DescriptorSource for BundledRegistrySource {
    fn resolve_calldata(
        &self,
        chain_id: u64,
        address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedDescriptor, ResolveError>> + Send + '_>> {
        let result = self.resolve_calldata_sync(chain_id, address);
        Box::pin(async move { result })
    }

    fn resolve_typed_candidates(
        &self,
        lookup: TypedDescriptorLookup,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResolvedDescriptor>, ResolveError>> + Send + '_>>
    {
        let result = self.resolve_typed_candidates_sync(&lookup);
        Box::pin(async move { result })
    }
}

fn parse_index<T: serde::de::DeserializeOwned>(rel_path: &str) -> Result<T, ResolveError> {
    let file =
        REGISTRY_SNAPSHOT
            .get_file(rel_path)
            .ok_or_else(|| ResolveError::RegistryIndexMissing {
                url: format!("bundled:{rel_path}"),
            })?;
    let body = file.contents_utf8().ok_or_else(|| {
        ResolveError::RegistryIo(format!("bundled index is not valid UTF-8: {rel_path}"))
    })?;
    serde_json::from_str(body).map_err(|e| ResolveError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONEINCH_ROUTER_V6: &str = "0x111111125421ca6dc452d289314280a0f8842a65";
    const BASE_USDC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
    const BASE_USDC_PERMIT_HASH: &str =
        "0x6e71edae12b1b97f4d1f60370fef10105fa2faae0126114a169c64845d6126c9";

    #[test]
    fn test_new_parses_embedded_indexes() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        assert!(!source.calldata_index.is_empty());
        assert!(!source.eip712_index.is_empty());
    }

    /// Snapshot integrity sweep: every path referenced by either embedded
    /// index must exist in the embedded tree, parse as a [`Descriptor`], and
    /// have its full `includes` chain resolvable in-snapshot.
    #[test]
    fn test_every_indexed_descriptor_loads() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");

        for path in source.calldata_index.values() {
            source
                .load_descriptor(path)
                .unwrap_or_else(|e| panic!("calldata descriptor {path} failed to load: {e}"));
        }

        for bucket in source.eip712_index.values() {
            for entries in bucket.values() {
                for entry in entries {
                    source.load_descriptor(&entry.path).unwrap_or_else(|e| {
                        panic!("eip712 descriptor {} failed to load: {e}", entry.path)
                    });
                }
            }
        }
    }

    #[tokio::test]
    async fn test_resolve_calldata_happy_path() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        let resolved = source
            .resolve_calldata(1, ONEINCH_ROUTER_V6)
            .await
            .expect("1inch AggregationRouterV6 must resolve from the bundled snapshot");
        assert!(!resolved.descriptor.display.formats.is_empty());
        assert_eq!(resolved.chain_id, 1);
        assert_eq!(resolved.address, ONEINCH_ROUTER_V6);
    }

    #[tokio::test]
    async fn test_resolve_calldata_is_address_case_insensitive() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        let resolved = source
            .resolve_calldata(1, &ONEINCH_ROUTER_V6.to_uppercase().replace("0X", "0x"))
            .await
            .expect("uppercase address must resolve");
        assert_eq!(resolved.address, ONEINCH_ROUTER_V6);
    }

    #[tokio::test]
    async fn test_resolve_calldata_unknown_address_not_found() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        let err = source
            .resolve_calldata(1, "0x000000000000000000000000000000000000dead")
            .await
            .expect_err("unknown address must not resolve");
        assert!(matches!(err, ResolveError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_resolve_typed_candidates_happy_path() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        let candidates = source
            .resolve_typed_candidates(TypedDescriptorLookup {
                chain_id: 8453,
                verifying_contract: BASE_USDC.to_string(),
                primary_type: "Permit".to_string(),
                encode_type_hash: Some(BASE_USDC_PERMIT_HASH.to_string()),
            })
            .await
            .expect("Base USDC Permit must resolve from the bundled snapshot");
        assert!(!candidates.is_empty());
        assert!(!candidates[0].descriptor.display.formats.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_typed_candidates_bogus_hash_not_found() {
        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        let err = source
            .resolve_typed_candidates(TypedDescriptorLookup {
                chain_id: 8453,
                verifying_contract: BASE_USDC.to_string(),
                primary_type: "Permit".to_string(),
                encode_type_hash: Some("0xdeadbeef".to_string()),
            })
            .await
            .expect_err("bogus encodeType hash must not match split index entries");
        assert!(matches!(err, ResolveError::NotFound { .. }));
    }

    /// The Base USDC permit descriptor carries no `display` of its own — its
    /// formats come entirely from the included `ercs/eip712-erc2612-permit.json`
    /// base. Loading it must transparently merge the includes chain.
    #[test]
    fn test_includes_chain_merges_base_formats() {
        let raw = BundledRegistrySource::read_raw("registry/permit/eip712-permit-base-usdc.json")
            .expect("raw descriptor file must exist");
        let raw_value: serde_json::Value = serde_json::from_str(raw).expect("raw JSON");
        assert!(
            raw_value.get("includes").is_some(),
            "test premise: raw file must use includes"
        );
        assert!(
            raw_value.get("display").is_none(),
            "test premise: raw file must not define display itself"
        );

        let source = BundledRegistrySource::new().expect("embedded indexes must parse");
        let descriptor = source
            .load_descriptor("registry/permit/eip712-permit-base-usdc.json")
            .expect("includes chain must resolve in-snapshot");
        assert!(
            !descriptor.display.formats.is_empty(),
            "merged descriptor must expose formats from the included base"
        );
    }
}
