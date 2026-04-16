use crate::engine::DisplayModel;
use crate::resolver::ResolvedDescriptor;
use std::ops::Deref;

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FormatDiagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderDiagnosticKind {
    InterpolatedIntentSkipped,
    DefinitionReferenceUnresolved,
    ValueUnresolved,
    TokenMetadataNotFound,
    TokenTickerNotFound,
    NftCollectionAddressMissing,
    NftCollectionNameNotFound,
    NestedCalldataDegraded,
    NestedDescriptorNotFound,
    NestedCalldataInvalidType,
    InteroperableAddressNameFallback,
    GenericRenderWarning,
}

impl RenderDiagnosticKind {
    fn code(self) -> &'static str {
        match self {
            Self::InterpolatedIntentSkipped => "interpolated_intent_skipped",
            Self::DefinitionReferenceUnresolved => "definition_reference_unresolved",
            Self::ValueUnresolved => "value_unresolved",
            Self::TokenMetadataNotFound => "token_metadata_not_found",
            Self::TokenTickerNotFound => "token_ticker_not_found",
            Self::NftCollectionAddressMissing => "nft_collection_address_missing",
            Self::NftCollectionNameNotFound => "nft_collection_name_not_found",
            Self::NestedCalldataDegraded => "nested_calldata_degraded",
            Self::NestedDescriptorNotFound => "nested_descriptor_not_found",
            Self::NestedCalldataInvalidType => "nested_calldata_invalid_type",
            Self::InteroperableAddressNameFallback => "interoperable_address_name_fallback",
            Self::GenericRenderWarning => "render_warning",
        }
    }

    fn severity(self) -> DiagnosticSeverity {
        DiagnosticSeverity::Warning
    }
}

pub(crate) fn render_warning(
    kind: RenderDiagnosticKind,
    message: impl Into<String>,
) -> FormatDiagnostic {
    FormatDiagnostic {
        code: kind.code().to_string(),
        severity: kind.severity(),
        message: message.into(),
    }
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum FallbackReason {
    DescriptorNotFound,
    FormatNotFound,
    NestedCallNotClearSigned,
    InsufficientContext,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, serde::Serialize)]
pub enum FormatOutcome {
    ClearSigned {
        model: DisplayModel,
        diagnostics: Vec<FormatDiagnostic>,
    },
    Fallback {
        model: DisplayModel,
        reason: FallbackReason,
        diagnostics: Vec<FormatDiagnostic>,
    },
}

impl FormatOutcome {
    pub fn model(&self) -> &DisplayModel {
        match self {
            Self::ClearSigned { model, .. } | Self::Fallback { model, .. } => model,
        }
    }

    pub fn diagnostics(&self) -> &[FormatDiagnostic] {
        match self {
            Self::ClearSigned { diagnostics, .. } | Self::Fallback { diagnostics, .. } => {
                diagnostics
            }
        }
    }

    pub fn fallback_reason(&self) -> Option<&FallbackReason> {
        match self {
            Self::ClearSigned { .. } => None,
            Self::Fallback { reason, .. } => Some(reason),
        }
    }

    pub fn is_clear_signed(&self) -> bool {
        matches!(self, Self::ClearSigned { .. })
    }

    pub fn into_model(self) -> DisplayModel {
        match self {
            Self::ClearSigned { model, .. } | Self::Fallback { model, .. } => model,
        }
    }
}

impl Deref for FormatOutcome {
    type Target = DisplayModel;

    fn deref(&self) -> &Self::Target {
        self.model()
    }
}

#[derive(Debug, Clone)]
pub enum ResolvedDescriptorResolution {
    Found(Vec<ResolvedDescriptor>),
    NotFound,
}

impl ResolvedDescriptorResolution {
    pub fn as_slice(&self) -> &[ResolvedDescriptor] {
        match self {
            Self::Found(descriptors) => descriptors,
            Self::NotFound => &[],
        }
    }
}

impl Deref for ResolvedDescriptorResolution {
    type Target = [ResolvedDescriptor];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum DescriptorResolutionOutcome {
    Found(Vec<String>),
    NotFound,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct RenderState {
    diagnostics: Vec<FormatDiagnostic>,
    fallback_reason: Option<FallbackReason>,
}

impl RenderState {
    pub(crate) fn diagnostics(&self) -> &[FormatDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn fallback_reason(&self) -> Option<FallbackReason> {
        self.fallback_reason.clone()
    }

    pub(crate) fn warn(&mut self, code: &str, message: impl Into<String>) {
        self.push(code, DiagnosticSeverity::Warning, message.into());
    }

    pub(crate) fn push_diagnostic(&mut self, diagnostic: FormatDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn mark_nested_fallback(&mut self) {
        if self.fallback_reason.is_none() {
            self.fallback_reason = Some(FallbackReason::NestedCallNotClearSigned);
        }
    }

    pub(crate) fn outcome(
        self,
        model: DisplayModel,
        fallback_reason: Option<FallbackReason>,
    ) -> FormatOutcome {
        let diagnostics = self.diagnostics;
        match fallback_reason.or(self.fallback_reason) {
            Some(reason) => FormatOutcome::Fallback {
                model,
                reason,
                diagnostics,
            },
            None => FormatOutcome::ClearSigned { model, diagnostics },
        }
    }

    fn push(&mut self, code: &str, severity: DiagnosticSeverity, message: String) {
        self.diagnostics.push(FormatDiagnostic {
            code: code.to_string(),
            severity,
            message,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{render_warning, RenderDiagnosticKind};

    #[test]
    fn render_warning_code_does_not_depend_on_message_text() {
        let first = render_warning(
            RenderDiagnosticKind::TokenMetadataNotFound,
            "token metadata not found for field 'Amount'",
        );
        let second = render_warning(
            RenderDiagnosticKind::TokenMetadataNotFound,
            "missing token metadata for field 'Amount'",
        );

        assert_eq!(first.code, "token_metadata_not_found");
        assert_eq!(second.code, "token_metadata_not_found");
    }

    #[test]
    fn render_warning_kinds_map_to_stable_codes() {
        let cases = [
            (
                RenderDiagnosticKind::InterpolatedIntentSkipped,
                "interpolated_intent_skipped",
            ),
            (
                RenderDiagnosticKind::DefinitionReferenceUnresolved,
                "definition_reference_unresolved",
            ),
            (RenderDiagnosticKind::ValueUnresolved, "value_unresolved"),
            (
                RenderDiagnosticKind::TokenMetadataNotFound,
                "token_metadata_not_found",
            ),
            (
                RenderDiagnosticKind::TokenTickerNotFound,
                "token_ticker_not_found",
            ),
            (
                RenderDiagnosticKind::NftCollectionAddressMissing,
                "nft_collection_address_missing",
            ),
            (
                RenderDiagnosticKind::NftCollectionNameNotFound,
                "nft_collection_name_not_found",
            ),
            (
                RenderDiagnosticKind::NestedCalldataDegraded,
                "nested_calldata_degraded",
            ),
            (
                RenderDiagnosticKind::NestedDescriptorNotFound,
                "nested_descriptor_not_found",
            ),
            (
                RenderDiagnosticKind::NestedCalldataInvalidType,
                "nested_calldata_invalid_type",
            ),
            (
                RenderDiagnosticKind::InteroperableAddressNameFallback,
                "interoperable_address_name_fallback",
            ),
        ];

        for (kind, expected_code) in cases {
            let diagnostic = render_warning(kind, "example");
            assert_eq!(diagnostic.code, expected_code);
        }
    }
}
