//! Error types for decoding, resolution, and rendering failures.

use thiserror::Error;

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, PartialEq, Eq, Error, serde::Serialize)]
pub enum FormatFailure {
    #[error("invalid input: {message}")]
    InvalidInput { message: String, retryable: bool },

    #[error("invalid descriptor: {message}")]
    InvalidDescriptor { message: String, retryable: bool },

    #[error("resolution failed: {message}")]
    ResolutionFailed { message: String, retryable: bool },

    #[error("internal error: {message}")]
    Internal { message: String, retryable: bool },
}

/// Unified error type for the ERC-7730 library.
#[derive(Debug, Error)]
pub enum Error {
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),

    #[error("descriptor error: {0}")]
    Descriptor(String),

    #[error("resolve error: {0}")]
    Resolve(#[from] ResolveError),

    #[error("token registry error: {0}")]
    TokenRegistry(String),

    #[error("render error: {0}")]
    Render(String),
}

/// Errors during signature parsing and calldata decoding.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("invalid function signature: {0}")]
    InvalidSignature(String),

    #[error("calldata too short: expected at least {expected} bytes, got {actual}")]
    CalldataTooShort { expected: usize, actual: usize },

    #[error("selector mismatch: expected {expected}, got {actual}")]
    SelectorMismatch { expected: String, actual: String },

    #[error("invalid ABI encoding: {0}")]
    InvalidEncoding(String),

    #[error("unsupported type: {0}")]
    UnsupportedType(String),
}

/// Errors during descriptor resolution.
#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("descriptor not found for chain_id={chain_id}, address={address}")]
    NotFound { chain_id: u64, address: String },

    #[error("registry index missing: {url}")]
    RegistryIndexMissing { url: String },

    #[error("registry descriptor missing: {url}")]
    RegistryDescriptorMissing { url: String },

    #[error("registry io error: {0}")]
    RegistryIo(String),

    #[error("parse error: {0}")]
    Parse(String),
}

impl From<Error> for FormatFailure {
    fn from(value: Error) -> Self {
        match value {
            Error::Decode(err) => Self::InvalidInput {
                message: err.to_string(),
                retryable: false,
            },
            Error::Descriptor(message) => Self::InvalidDescriptor {
                message,
                retryable: false,
            },
            Error::Resolve(err) => err.into(),
            Error::TokenRegistry(message) => Self::ResolutionFailed {
                message: format!("token registry error: {message}"),
                retryable: true,
            },
            Error::Render(message) => Self::InvalidDescriptor {
                message,
                retryable: false,
            },
        }
    }
}

impl From<ResolveError> for FormatFailure {
    fn from(value: ResolveError) -> Self {
        match value {
            ResolveError::NotFound { chain_id, address } => Self::InvalidDescriptor {
                message: format!("descriptor not found for chain_id={chain_id}, address={address}"),
                retryable: false,
            },
            ResolveError::RegistryIndexMissing { url } => Self::ResolutionFailed {
                message: format!("registry index missing: {url}"),
                retryable: true,
            },
            ResolveError::RegistryDescriptorMissing { url } => Self::ResolutionFailed {
                message: format!("registry descriptor missing: {url}"),
                retryable: true,
            },
            ResolveError::RegistryIo(message) => Self::ResolutionFailed {
                message: format!("registry io error: {message}"),
                retryable: true,
            },
            ResolveError::Parse(message) => Self::ResolutionFailed {
                message: format!("parse error: {message}"),
                retryable: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_failure_from_error_decode() {
        let f = FormatFailure::from(Error::Decode(DecodeError::CalldataTooShort {
            expected: 4,
            actual: 2,
        }));
        match f {
            FormatFailure::InvalidInput { message, retryable } => {
                assert!(message.contains("calldata too short"));
                assert!(!retryable);
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_error_descriptor() {
        let f = FormatFailure::from(Error::Descriptor("bad".into()));
        match f {
            FormatFailure::InvalidDescriptor { message, retryable } => {
                assert_eq!(message, "bad");
                assert!(!retryable);
            }
            other => panic!("expected InvalidDescriptor, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_error_resolve_forwards() {
        let f = FormatFailure::from(Error::Resolve(ResolveError::NotFound {
            chain_id: 1,
            address: "0xabc".into(),
        }));
        match f {
            FormatFailure::InvalidDescriptor { message, retryable } => {
                assert!(message.contains("chain_id=1"));
                assert!(message.contains("0xabc"));
                assert!(!retryable);
            }
            other => panic!("expected InvalidDescriptor, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_error_token_registry() {
        let f = FormatFailure::from(Error::TokenRegistry("rate limit".into()));
        match f {
            FormatFailure::ResolutionFailed { message, retryable } => {
                assert!(message.starts_with("token registry error:"));
                assert!(message.contains("rate limit"));
                assert!(retryable);
            }
            other => panic!("expected ResolutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_error_render() {
        let f = FormatFailure::from(Error::Render("nope".into()));
        match f {
            FormatFailure::InvalidDescriptor { message, retryable } => {
                assert_eq!(message, "nope");
                assert!(!retryable);
            }
            other => panic!("expected InvalidDescriptor, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_resolve_not_found() {
        let f = FormatFailure::from(ResolveError::NotFound {
            chain_id: 137,
            address: "0xdead".into(),
        });
        match f {
            FormatFailure::InvalidDescriptor { message, retryable } => {
                assert!(message.contains("chain_id=137"));
                assert!(message.contains("0xdead"));
                assert!(!retryable);
            }
            other => panic!("expected InvalidDescriptor, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_resolve_index_missing() {
        let f = FormatFailure::from(ResolveError::RegistryIndexMissing {
            url: "https://example/idx".into(),
        });
        match f {
            FormatFailure::ResolutionFailed { message, retryable } => {
                assert!(message.contains("registry index missing"));
                assert!(message.contains("https://example/idx"));
                assert!(retryable);
            }
            other => panic!("expected ResolutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_resolve_descriptor_missing() {
        let f = FormatFailure::from(ResolveError::RegistryDescriptorMissing {
            url: "https://example/d.json".into(),
        });
        match f {
            FormatFailure::ResolutionFailed { message, retryable } => {
                assert!(message.contains("registry descriptor missing"));
                assert!(retryable);
            }
            other => panic!("expected ResolutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_resolve_io() {
        let f = FormatFailure::from(ResolveError::RegistryIo("timeout".into()));
        match f {
            FormatFailure::ResolutionFailed { message, retryable } => {
                assert!(message.contains("registry io error"));
                assert!(message.contains("timeout"));
                assert!(retryable);
            }
            other => panic!("expected ResolutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn format_failure_from_resolve_parse() {
        let f = FormatFailure::from(ResolveError::Parse("bad json".into()));
        match f {
            FormatFailure::ResolutionFailed { message, retryable } => {
                assert!(message.contains("parse error"));
                assert!(message.contains("bad json"));
                assert!(!retryable);
            }
            other => panic!("expected ResolutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn decode_error_display() {
        assert!(DecodeError::InvalidSignature("foo".into())
            .to_string()
            .contains("invalid function signature"));
        assert!(DecodeError::SelectorMismatch {
            expected: "a".into(),
            actual: "b".into(),
        }
        .to_string()
        .contains("selector mismatch"));
        assert!(DecodeError::InvalidEncoding("e".into())
            .to_string()
            .contains("invalid ABI encoding"));
        assert!(DecodeError::UnsupportedType("t".into())
            .to_string()
            .contains("unsupported type"));
    }

    #[test]
    fn resolve_error_display() {
        assert!(ResolveError::NotFound {
            chain_id: 1,
            address: "x".into(),
        }
        .to_string()
        .contains("chain_id=1"));
        assert!(ResolveError::RegistryIndexMissing { url: "u".into() }
            .to_string()
            .contains("index missing"));
        assert!(ResolveError::RegistryDescriptorMissing { url: "u".into() }
            .to_string()
            .contains("descriptor missing"));
        assert!(ResolveError::RegistryIo("e".into())
            .to_string()
            .contains("io error"));
        assert!(ResolveError::Parse("p".into())
            .to_string()
            .contains("parse error"));
    }

    #[test]
    fn error_display() {
        assert!(Error::Descriptor("d".into())
            .to_string()
            .contains("descriptor error"));
        assert!(Error::TokenRegistry("t".into())
            .to_string()
            .contains("token registry"));
        assert!(Error::Render("r".into())
            .to_string()
            .contains("render error"));
    }
}
