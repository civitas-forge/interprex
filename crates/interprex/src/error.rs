use std::fmt;

use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ModelError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} must not contain '/' or ASCII control characters")]
    InvalidSegment { field: &'static str },
    #[error("repository must have the form owner/name")]
    InvalidRepository,
    #[error("number must be greater than zero")]
    InvalidNumber,
    #[error(
        "{field} must begin and end with an ASCII letter or digit and contain only ASCII letters, digits, '.', '_' or single '-' characters"
    )]
    InvalidProviderTextIdentifier { field: &'static str },
    #[error("provider text record value must contain a positive integer 'version' field")]
    InvalidProviderTextRecordVersion,
    #[error("reviewer application actor must be a bot")]
    ReviewerApplicationActorNotBot,
    #[error("required check {name} appears more than once")]
    DuplicateRequiredCheck { name: String },
}

pub(crate) fn segment(
    value: impl Into<String>,
    field: &'static str,
) -> std::result::Result<String, ModelError> {
    let value = value.into();
    if value.is_empty() {
        return Err(ModelError::Empty { field });
    }
    if value.contains('/') || value.chars().any(char::is_control) {
        return Err(ModelError::InvalidSegment { field });
    }
    Ok(value)
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderError {
    #[error("{provider} does not support {operation}")]
    Unsupported {
        provider: &'static str,
        operation: &'static str,
    },
    #[error("unrepresentable {provider} data: {fact}")]
    Unrepresentable {
        provider: &'static str,
        fact: String,
    },
    #[error("{entity} was not found")]
    NotFound { entity: String },
    /// A credential the operation needs is absent from the configuration the
    /// provider was built from. `entry` names the declaration that would
    /// supply it, so the message a stuck caller reads points at the place to
    /// edit rather than only at what was missing.
    #[error("missing {kind} credential for identity {identity}: {entry} is absent from {origin}")]
    MissingCredential {
        identity: String,
        kind: &'static str,
        entry: String,
        origin: ConfigurationSource,
    },
    #[error("provider configuration from {origin} failed: {reason}")]
    Configuration {
        origin: ConfigurationSource,
        reason: String,
    },
    #[error("{provider} {operation} failed: {message}")]
    External {
        provider: &'static str,
        operation: &'static str,
        message: String,
    },
    /// The caller's request contradicts itself; correcting the request, not
    /// retrying it, resolves this error.
    #[error("invalid input for {provider}: {fact}")]
    InvalidInput {
        provider: &'static str,
        fact: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationSource {
    Direct,
    File(String),
}

impl fmt::Display for ConfigurationSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct => formatter.write_str("direct construction"),
            Self::File(path) => write!(formatter, "file {path}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, ProviderError>;
