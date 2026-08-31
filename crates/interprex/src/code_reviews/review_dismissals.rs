use serde::{Deserialize, Serialize};

use crate::ModelError;

/// The visible reason recorded with one review dismissal.
///
/// GitHub requires text with a dismissal, so a blank message is not
/// constructible rather than refused at the provider boundary. The provider
/// records the text exactly as supplied.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReviewDismissalMessage(String);

impl ReviewDismissalMessage {
    /// Constructs a nonblank dismissal message.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Empty`] when `value` contains no visible text.
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::Empty {
                field: "review dismissal message",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ReviewDismissalMessage {
    type Error = ModelError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ReviewDismissalMessage> for String {
    fn from(value: ReviewDismissalMessage) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismissal_message_rejects_blank_text_during_construction_and_deserialization() {
        assert_eq!(
            ReviewDismissalMessage::new(" \n\t"),
            Err(ModelError::Empty {
                field: "review dismissal message"
            })
        );
        assert!(serde_json::from_value::<ReviewDismissalMessage>(serde_json::json!(" ")).is_err());
    }

    #[test]
    fn dismissal_message_round_trips_through_its_public_format() {
        let message = ReviewDismissalMessage::new("Round 1 concluded.").expect("message");
        let encoded = serde_json::to_value(&message).expect("serialize message");
        assert_eq!(encoded, serde_json::json!("Round 1 concluded."));
        assert_eq!(
            serde_json::from_value::<ReviewDismissalMessage>(encoded).expect("deserialize message"),
            message
        );
    }
}
