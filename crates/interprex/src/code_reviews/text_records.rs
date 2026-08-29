use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ModelError;

/// One application-owned JSON value carried inside provider text.
///
/// The namespace and record name each begin and end with an ASCII letter or
/// digit. Between those ends they may also contain `.`, `_` and single `-`
/// characters. Those rules let provider adapters place the identifiers in a
/// delimited text marker without escaping them. `value` is a JSON object
/// containing a positive integer `version` field. Construction and
/// deserialization apply all three rules.
///
/// Interprex reads only that version field. A caller decides what the remaining
/// JSON members, namespace, record name and version mean.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "SerializedProviderTextRecord",
    into = "SerializedProviderTextRecord"
)]
pub struct ProviderTextRecord {
    namespace: String,
    name: String,
    version: u64,
    value: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SerializedProviderTextRecord {
    namespace: String,
    name: String,
    value: Value,
}

impl ProviderTextRecord {
    /// Constructs one record after validating its carrier identifiers and
    /// protocol version.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Empty`] for an empty namespace or record name,
    /// [`ModelError::InvalidProviderTextIdentifier`] when either identifier
    /// contains characters reserved by the carrier, and
    /// [`ModelError::InvalidProviderTextRecordVersion`] when `value` is not an
    /// object with a positive integer `version` member.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        value: Value,
    ) -> std::result::Result<Self, ModelError> {
        let namespace = provider_text_identifier(namespace, "provider text record namespace")?;
        let name = provider_text_identifier(name, "provider text record name")?;
        let version = value
            .get("version")
            .and_then(Value::as_u64)
            .filter(|version| *version > 0)
            .ok_or(ModelError::InvalidProviderTextRecordVersion)?;
        Ok(Self {
            namespace,
            name,
            version,
            value,
        })
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// The application-owned JSON value.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }
}

impl TryFrom<SerializedProviderTextRecord> for ProviderTextRecord {
    type Error = ModelError;

    fn try_from(value: SerializedProviderTextRecord) -> std::result::Result<Self, Self::Error> {
        Self::new(value.namespace, value.name, value.value)
    }
}

impl From<ProviderTextRecord> for SerializedProviderTextRecord {
    fn from(value: ProviderTextRecord) -> Self {
        Self {
            namespace: value.namespace,
            name: value.name,
            value: value.value,
        }
    }
}

fn provider_text_identifier(
    value: impl Into<String>,
    field: &'static str,
) -> std::result::Result<String, ModelError> {
    let value = value.into();
    if value.is_empty() {
        return Err(ModelError::Empty { field });
    }
    let valid_edge = |character: char| character.is_ascii_alphanumeric();
    let valid_character =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-');
    if !value.starts_with(valid_edge)
        || !value.ends_with(valid_edge)
        || !value.chars().all(valid_character)
        || value.contains("--")
    {
        return Err(ModelError::InvalidProviderTextIdentifier { field });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_without_interpreting_its_json_value() {
        let record = ProviderTextRecord::new(
            "comitia",
            "loop-event",
            serde_json::json!({"version": 3, "kind": "round-planned", "round": 4}),
        )
        .expect("valid record");

        let encoded = serde_json::to_value(&record).expect("serialize record");
        assert_eq!(
            encoded,
            serde_json::json!({
                "namespace": "comitia",
                "name": "loop-event",
                "value": {"version": 3, "kind": "round-planned", "round": 4}
            })
        );
        assert_eq!(
            serde_json::from_value::<ProviderTextRecord>(encoded).expect("deserialize record"),
            record
        );
        assert_eq!(record.version(), 3);
    }

    #[test]
    fn record_rejects_identifiers_that_can_conflict_with_carrier_delimiters() {
        for invalid in [
            "-comitia",
            "comitia-",
            "comitia--events",
            "comitia:events",
            "comitia events",
            "comitia/events",
            "comitia\n",
            "comitia→events",
        ] {
            assert!(
                matches!(
                    ProviderTextRecord::new(
                        invalid,
                        "loop-event",
                        serde_json::json!({"version": 1})
                    ),
                    Err(ModelError::InvalidProviderTextIdentifier { .. })
                ),
                "{invalid:?}"
            );
        }
        assert_eq!(
            ProviderTextRecord::new("", "loop-event", serde_json::json!({"version": 1})),
            Err(ModelError::Empty {
                field: "provider text record namespace"
            })
        );
    }

    #[test]
    fn record_requires_a_positive_protocol_version_during_construction_and_deserialization() {
        assert_eq!(
            ProviderTextRecord::new("comitia", "loop-event", serde_json::json!({"version": 0})),
            Err(ModelError::InvalidProviderTextRecordVersion)
        );
        let encoded = serde_json::json!({
            "namespace": "comitia",
            "name": "loop-event",
            "value": {"version": 0}
        });
        assert!(serde_json::from_value::<ProviderTextRecord>(encoded).is_err());

        for value in [
            Value::Null,
            serde_json::json!({}),
            serde_json::json!({"version": -1}),
            serde_json::json!({"version": "1"}),
        ] {
            assert_eq!(
                ProviderTextRecord::new("comitia", "loop-event", value),
                Err(ModelError::InvalidProviderTextRecordVersion)
            );
        }
    }
}
