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
