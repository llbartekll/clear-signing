//! Pluggable descriptor resolution facade.
//!
//! The public `crate::resolver` module re-exports the stable resolution surface
//! while delegating implementation to focused private submodules.

mod nested_resolution;
mod source;
mod standard_token;
mod typed_selection;

#[cfg(feature = "bundled-registry")]
mod bundled_registry;
#[cfg(feature = "github-registry")]
mod github_registry;
#[cfg(any(feature = "github-registry", feature = "bundled-registry"))]
mod registry_common;

#[cfg(test)]
pub(crate) mod test_support;

pub use nested_resolution::{resolve_descriptors_for_tx, resolve_descriptors_for_typed_data};
pub use source::{DescriptorSource, ResolvedDescriptor, StaticSource, TypedDescriptorLookup};

#[cfg(feature = "bundled-registry")]
pub use bundled_registry::BundledRegistrySource;
#[cfg(feature = "github-registry")]
pub use github_registry::GitHubRegistrySource;
#[cfg(any(feature = "github-registry", feature = "bundled-registry"))]
pub use registry_common::Eip712IndexEntry;

pub(crate) use typed_selection::{select_typed_outer_descriptor, TypedOuterSelection};
