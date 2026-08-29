use interprex::{ProviderError, Repository, Result, ReviewActorKind, ReviewerApplication};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use crate::{
    GithubProvider,
    client::{external, is_not_found},
};

use super::actors::{GithubApp, GithubUser, normalize_app, rest_actor};

const RESOLVE_APPLICATION: &str = "resolve reviewer application";

impl GithubProvider {
    pub(super) async fn github_reviewer_application(
        &self,
        _repository: &Repository,
        slug: &str,
    ) -> Result<ReviewerApplication> {
        let requested_slug = utf8_percent_encode(slug, NON_ALPHANUMERIC);
        let app_entity = format!("reviewer application {slug}");
        let app: GithubApp = self
            .user()?
            .get(format!("/apps/{requested_slug}"), None::<&()>)
            .await
            .map_err(|error| application_read_error(app_entity, error))?;
        let app = normalize_app(app)?;
        if app.slug.is_empty() {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("reviewer application {slug} has an empty canonical slug"),
            });
        }

        let bot_login = format!("{}[bot]", app.slug);
        let bot_segment = utf8_percent_encode(&bot_login, NON_ALPHANUMERIC);
        let bot_entity = format!("reviewer application bot {bot_login}");
        let bot: GithubUser = self
            .user()?
            .get(format!("/users/{bot_segment}"), None::<&()>)
            .await
            .map_err(|error| application_read_error(bot_entity, error))?;
        let bot = rest_actor(bot)?;

        if bot.kind != ReviewActorKind::Bot {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!(
                    "reviewer application actor {} must be a bot, but GitHub reported {:?}",
                    bot.login, bot.kind
                ),
            });
        }
        ReviewerApplication::new(app, bot).map_err(|error| ProviderError::Unrepresentable {
            provider: "github",
            fact: error.to_string(),
        })
    }
}

fn application_read_error(entity: String, error: octocrab::Error) -> ProviderError {
    if is_not_found(&error) {
        ProviderError::NotFound { entity }
    } else if matches!(
        &error,
        octocrab::Error::Json { .. } | octocrab::Error::Serde { .. }
    ) {
        ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("{entity} response could not be decoded: {error}"),
        }
    } else {
        external(RESOLVE_APPLICATION, error)
    }
}
