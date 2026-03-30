//! Pluggable descriptor resolution facade.
//!
//! The public `crate::resolver` module re-exports the stable resolution surface
//! while delegating implementation to focused private submodules.

mod nested_resolution;
mod source;
mod typed_selection;

#[cfg(feature = "github-registry")]
mod github_registry;

#[cfg(test)]
pub(crate) mod test_support;

pub use nested_resolution::{resolve_descriptors_for_tx, resolve_descriptors_for_typed_data};
pub use source::{DescriptorSource, ResolvedDescriptor, StaticSource, TypedDescriptorLookup};

#[cfg(feature = "github-registry")]
pub use github_registry::{Eip712IndexEntry, GitHubRegistrySource};

pub(crate) use typed_selection::{select_typed_outer_descriptor, TypedOuterSelection};
