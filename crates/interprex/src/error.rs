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
    #[error("unrepresentable {provider} data: {fact}")]
    Unrepresentable {
        provider: &'static str,
        fact: String,
    },
    #[error("{entity} was not found")]
    NotFound { entity: String },
    #[error("missing {kind} credential for identity {identity}")]
    MissingCredential {
        identity: String,
        kind: &'static str,
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
