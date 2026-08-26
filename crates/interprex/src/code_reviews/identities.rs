use serde::{Deserialize, Serialize};

macro_rules! opaque_review_id {
    ($name:ident, $field:literal, $entity:literal) => {
        #[doc = concat!("Opaque provider identifier for a ", $entity, ".")]
        ///
        /// Consumers retain this value only to address the same entity
        /// through the provider that returned it. Its representation has no
        /// provider-neutral meaning.
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> std::result::Result<Self, crate::ModelError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(crate::ModelError::Empty { field: $field });
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

opaque_review_id!(ReviewId, "review id", "review");
opaque_review_id!(ReviewThreadId, "review thread id", "review thread");
opaque_review_id!(ReviewCommentId, "review comment id", "review comment");
opaque_review_id!(ReviewRequestId, "review request id", "review request");
opaque_review_id!(ReviewActorId, "review actor id", "review actor");
opaque_review_id!(ReviewTeamId, "review team id", "review team");
opaque_review_id!(ProviderAppId, "provider app id", "provider application");

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewActorKind {
    User,
    Bot,
    Placeholder,
    Organization,
    EnterpriseUser,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReviewActor {
    pub id: ReviewActorId,
    pub login: String,
    pub kind: ReviewActorKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTeamKind {
    Organization,
    Enterprise,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewTeam {
    pub id: ReviewTeamId,
    pub slug: String,
    pub name: String,
    pub kind: ReviewTeamKind,
}

/// The GitHub App or equivalent provider application that produced a review or
/// published a check.
///
/// This is attribution. It is neither the actor a review is credited to nor
/// the identity a provider authenticated as.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderApp {
    pub id: ProviderAppId,
    pub slug: String,
    pub name: String,
}
