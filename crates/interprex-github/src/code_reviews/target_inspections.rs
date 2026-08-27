use interprex::{
    ProviderError, Repository, Result, ReviewRequestTarget, ReviewRequestTargetInspection,
    ReviewTarget, ReviewTeam, ReviewTeamId, ReviewTeamKind,
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

use crate::{GithubProvider, client::external, client::is_not_found};

use super::actors::{GithubUser, rest_actor};

const INSPECT_TARGET: &str = "inspect review request target";

#[derive(Deserialize)]
struct GithubTeam {
    node_id: String,
    slug: String,
    name: String,
}

impl GithubProvider {
    pub(super) async fn github_review_request_target(
        &self,
        _repository: &Repository,
        target: &ReviewRequestTarget,
    ) -> Result<ReviewRequestTargetInspection> {
        let observed = match target {
            ReviewRequestTarget::User(login) => {
                self.github_actor(user_lookup_candidates(login)).await?
            }
            ReviewRequestTarget::Bot(login) => {
                self.github_actor(bot_lookup_candidates(login)).await?
            }
            ReviewRequestTarget::Team(identifier) => self.github_team(identifier).await?,
        };
        Ok(
            observed.map_or(ReviewRequestTargetInspection::NotResolvable, |observed| {
                ReviewRequestTargetInspection::from_observation(target, observed)
            }),
        )
    }

    async fn github_actor(&self, candidates: Vec<String>) -> Result<Option<ReviewTarget>> {
        for login in candidates {
            let segment = utf8_percent_encode(&login, NON_ALPHANUMERIC);
            let response: std::result::Result<GithubUser, octocrab::Error> = self
                .user()?
                .get(format!("/users/{segment}"), None::<&()>)
                .await;
            match response {
                Ok(user) => return rest_actor(user).map(ReviewTarget::Actor).map(Some),
                Err(error) if is_not_found(&error) => {}
                Err(error) => return Err(external(INSPECT_TARGET, error)),
            }
        }
        Ok(None)
    }

    async fn github_team(&self, identifier: &str) -> Result<Option<ReviewTarget>> {
        let (organization, slug) = parse_team_identifier(identifier)?;
        let organization = utf8_percent_encode(organization, NON_ALPHANUMERIC);
        let slug = utf8_percent_encode(slug, NON_ALPHANUMERIC);
        let response: std::result::Result<GithubTeam, octocrab::Error> = self
            .user()?
            .get(format!("/orgs/{organization}/teams/{slug}"), None::<&()>)
            .await;
        match response {
            Ok(team) => Ok(Some(ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new(team.node_id).map_err(|error| {
                    ProviderError::Unrepresentable {
                        provider: "github",
                        fact: error.to_string(),
                    }
                })?,
                slug: team.slug,
                name: team.name,
                kind: ReviewTeamKind::Organization,
            }))),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(external(INSPECT_TARGET, error)),
        }
    }
}

fn user_lookup_candidates(login: &str) -> Vec<String> {
    let mut candidates = vec![login.to_owned()];
    if !login.ends_with("[bot]") {
        candidates.push(format!("{login}[bot]"));
    }
    candidates
}

fn bot_lookup_candidates(login: &str) -> Vec<String> {
    match login.strip_suffix("[bot]") {
        Some(stem) => vec![login.to_owned(), stem.to_owned()],
        None => vec![format!("{login}[bot]"), login.to_owned()],
    }
}

fn parse_team_identifier(identifier: &str) -> Result<(&str, &str)> {
    let mut segments = identifier.split('/');
    let organization = segments.next().unwrap_or_default();
    let slug = segments.next().unwrap_or_default();
    if organization.is_empty() || slug.is_empty() || segments.next().is_some() {
        return Err(ProviderError::InvalidInput {
            provider: "github",
            fact: format!(
                "review team target `{identifier}` must have the form organization/team-slug"
            ),
        });
    }
    Ok((organization, slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_order_preserves_the_requested_lookup_rules() {
        assert_eq!(
            user_lookup_candidates("reviewer"),
            ["reviewer", "reviewer[bot]"]
        );
        assert_eq!(user_lookup_candidates("reviewer[bot]"), ["reviewer[bot]"]);
        assert_eq!(
            bot_lookup_candidates("reviewer"),
            ["reviewer[bot]", "reviewer"]
        );
        assert_eq!(
            bot_lookup_candidates("reviewer[bot]"),
            ["reviewer[bot]", "reviewer"]
        );
    }

    #[test]
    fn team_identifiers_have_two_nonempty_segments() {
        assert_eq!(
            parse_team_identifier("civitas-forge/maintainers").expect("team identifier"),
            ("civitas-forge", "maintainers")
        );
        for invalid in ["maintainers", "/maintainers", "civitas-forge/", "a/b/c"] {
            assert!(matches!(
                parse_team_identifier(invalid),
                Err(ProviderError::InvalidInput { .. })
            ));
        }
    }
}
