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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_source_not_found() {
        let source = StaticSource::new();
        let result = source.resolve_calldata(1, "0xabc").await;
        assert!(result.is_err());
    }
}
