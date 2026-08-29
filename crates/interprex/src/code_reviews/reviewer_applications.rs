use serde::{Deserialize, Serialize};

use super::{ProviderApp, ReviewActor, ReviewActorKind};
use crate::ModelError;

/// A provider application and the bot actor that authors its reviews.
///
/// Construction and deserialization require `bot.kind` to be
/// [`ReviewActorKind::Bot`]. This pairing identifies the actor whose reviews a
/// caller can recognize. It does not establish that the application is
/// installed in a repository, that the provider can register the actor as a
/// requested reviewer, or that a delivered review will carry the application
/// as separate attribution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "SerializedReviewerApplication",
    into = "SerializedReviewerApplication"
)]
pub struct ReviewerApplication {
    app: ProviderApp,
    bot: ReviewActor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SerializedReviewerApplication {
    app: ProviderApp,
    bot: ReviewActor,
}

impl ReviewerApplication {
    /// Pairs an application with the bot actor that authors its reviews.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::ReviewerApplicationActorNotBot`] when the actor's
    /// kind is not [`ReviewActorKind::Bot`].
    pub fn new(app: ProviderApp, bot: ReviewActor) -> std::result::Result<Self, ModelError> {
        if bot.kind != ReviewActorKind::Bot {
            return Err(ModelError::ReviewerApplicationActorNotBot);
        }
        Ok(Self { app, bot })
    }

    #[must_use]
    pub const fn app(&self) -> &ProviderApp {
        &self.app
    }

    #[must_use]
    pub const fn bot(&self) -> &ReviewActor {
        &self.bot
    }

    /// Whether two observations identify the same application and bot actor.
    ///
    /// Provider application names and slugs and bot logins can change. Their
    /// provider-assigned IDs define identity and remain the only fields this
    /// comparison reads.
    #[must_use]
    pub fn same_identity_as(&self, other: &Self) -> bool {
        self.app.id == other.app.id && self.bot.id == other.bot.id
    }
}

impl TryFrom<SerializedReviewerApplication> for ReviewerApplication {
    type Error = ModelError;

    fn try_from(value: SerializedReviewerApplication) -> std::result::Result<Self, Self::Error> {
        Self::new(value.app, value.bot)
    }
}

impl From<ReviewerApplication> for SerializedReviewerApplication {
    fn from(value: ReviewerApplication) -> Self {
        Self {
            app: value.app,
            bot: value.bot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProviderAppId, ReviewActorId};

    fn app() -> ProviderApp {
        ProviderApp {
            id: ProviderAppId::new("4111233").expect("app id"),
            slug: "adr-codex-review".to_owned(),
            name: "ADR Codex Review".to_owned(),
        }
    }

    fn actor(kind: ReviewActorKind) -> ReviewActor {
        ReviewActor {
            id: ReviewActorId::new("BOT_kgDOEZ_BKw").expect("actor id"),
            login: "adr-codex-review[bot]".to_owned(),
            kind,
        }
    }

    #[test]
    fn reviewer_application_round_trips_its_distinct_app_and_bot_identities() {
        let application =
            ReviewerApplication::new(app(), actor(ReviewActorKind::Bot)).expect("bot application");
        assert_eq!(application.app().slug, "adr-codex-review");
        assert_eq!(application.bot().login, "adr-codex-review[bot]");

        let encoded = serde_json::to_value(&application).expect("serialize application");
        assert_eq!(
            serde_json::from_value::<ReviewerApplication>(encoded)
                .expect("deserialize application"),
            application
        );
    }

    #[test]
    fn reviewer_application_requires_a_bot_during_construction_and_deserialization() {
        assert_eq!(
            ReviewerApplication::new(app(), actor(ReviewActorKind::User)),
            Err(ModelError::ReviewerApplicationActorNotBot)
        );
        let encoded = serde_json::json!({
            "app": {
                "id": "4111233",
                "slug": "adr-codex-review",
                "name": "ADR Codex Review"
            },
            "bot": {
                "id": "U_kgDOEZ_BKw",
                "login": "someone",
                "kind": "user"
            }
        });
        assert!(serde_json::from_value::<ReviewerApplication>(encoded).is_err());
    }

    #[test]
    fn reviewer_application_identity_uses_only_provider_assigned_ids() {
        let original =
            ReviewerApplication::new(app(), actor(ReviewActorKind::Bot)).expect("application");
        let renamed = ReviewerApplication::new(
            ProviderApp {
                id: original.app().id.clone(),
                slug: "renamed-reviewer".to_owned(),
                name: "Renamed Reviewer".to_owned(),
            },
            ReviewActor {
                id: original.bot().id.clone(),
                login: "renamed-reviewer[bot]".to_owned(),
                kind: ReviewActorKind::Bot,
            },
        )
        .expect("renamed application");
        assert!(original.same_identity_as(&renamed));

        let different_app = ReviewerApplication::new(
            ProviderApp {
                id: ProviderAppId::new("other-app").expect("app id"),
                ..app()
            },
            actor(ReviewActorKind::Bot),
        )
        .expect("different application");
        assert!(!original.same_identity_as(&different_app));
    }
}
