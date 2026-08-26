use interprex::{
    ProviderApp, ProviderAppId, ProviderError, Result, ReviewActor, ReviewActorId, ReviewActorKind,
};
use serde::Deserialize;

#[derive(Deserialize, PartialEq)]
pub(super) struct GithubUser {
    pub(super) node_id: String,
    pub(super) login: String,
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
}

#[derive(Deserialize, PartialEq)]
pub(super) struct GithubApp {
    id: u64,
    slug: String,
    name: String,
}

pub(super) fn actor(id: String, login: String, kind: &str) -> Result<ReviewActor> {
    let kind = match kind {
        "User" => ReviewActorKind::User,
        "Bot" => ReviewActorKind::Bot,
        "Mannequin" => ReviewActorKind::Placeholder,
        "Organization" => ReviewActorKind::Organization,
        "EnterpriseUserAccount" => ReviewActorKind::EnterpriseUser,
        other => {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("unknown review actor kind {other}"),
            });
        }
    };
    Ok(ReviewActor {
        id: ReviewActorId::new(id).map_err(|error| ProviderError::Unrepresentable {
            provider: "github",
            fact: error.to_string(),
        })?,
        login,
        kind,
    })
}

pub(super) fn normalize_app(app: GithubApp) -> Result<ProviderApp> {
    Ok(ProviderApp {
        id: ProviderAppId::new(app.id.to_string()).map_err(|error| {
            ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
            }
        })?,
        slug: app.slug,
        name: app.name,
    })
}

pub(super) fn rest_actor(user: GithubUser) -> Result<ReviewActor> {
    let kind = user.kind.ok_or_else(|| ProviderError::Unrepresentable {
        provider: "github",
        fact: format!("actor {} has no type", user.login),
    })?;
    actor(user.node_id, user.login, &kind)
}

pub(super) fn ghost_actor(id: String) -> Result<ReviewActor> {
    Ok(ReviewActor {
        id: ReviewActorId::new(id).map_err(|error| ProviderError::Unrepresentable {
            provider: "github",
            fact: error.to_string(),
        })?,
        login: "ghost".to_owned(),
        kind: ReviewActorKind::Placeholder,
    })
}

#[cfg(test)]
mod tests {
    use interprex::ProviderError;

    use super::super::change_requests::{
        GithubPullRequest, GithubReview, normalize_change_request,
    };

    #[test]
    fn unknown_actor_kinds_are_unrepresentable() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let mut reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
            "../../tests/fixtures/code_review_reviews.json"
        ))
        .expect("review fixture");
        reviews[0].user.as_mut().expect("reviewer").kind = Some("Repository".to_owned());

        let error = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("unknown actor kind must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("unknown review actor kind")
        ));
    }
    #[test]
    fn actors_without_a_type_are_unrepresentable() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let mut reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
            "../../tests/fixtures/code_review_reviews.json"
        ))
        .expect("review fixture");
        reviews[0].user.as_mut().expect("reviewer").kind = None;

        let error = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("missing actor type must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has no type")
        ));
    }
}
