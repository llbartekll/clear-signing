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
                message: format!(
                    "descriptor not found for chain_id={chain_id}, address={address}"
                ),
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
