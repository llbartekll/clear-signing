//! Top-level [`Descriptor`] type — the root of an ERC-7730 JSON descriptor.

use serde::{Deserialize, Serialize};

use super::context::DescriptorContext;
use super::display::DescriptorDisplay;
use super::metadata::Metadata;

/// Top-level ERC-7730 v2 descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    #[serde(rename = "$schema")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// URL of an included descriptor to merge with this one.
    /// Consumed during resolution — should not be present at formatting time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub includes: Option<String>,

    pub context: DescriptorContext,

    pub metadata: Metadata,

    pub display: DescriptorDisplay,
}

impl Descriptor {
    /// Parse a descriptor from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
