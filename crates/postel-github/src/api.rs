//! Provider construction from typed or project configuration.
//!
//! Direct and file construction converge before clients are built. Secrets are
//! stored in `SecretString`, and the custom debug views report only presence and
//! identity names. A named app client is installation-scoped immediately, but
//! Octocrab does not fetch its installation token until the first request.

use std::{collections::BTreeMap, fmt, path::Path, sync::Arc};

use jsonwebtoken::EncodingKey;
use octocrab::{
    DefaultOctocrabBuilderConfig, NoAuth, NoSvc, NotLayerReady, Octocrab, OctocrabBuilder,
    service::middleware::retry::{NoOpRateLimitMetrics, RetryConfig},
};
use postel_contracts::{ProviderError, Result};
use postel_sys::{RealSystem, System};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

#[derive(Clone)]
pub struct AppCredentials {
    pub app_id: u64,
    pub installation_id: u64,
    pub private_key: SecretString,
}

impl fmt::Debug for AppCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppCredentials")
            .field("app_id", &self.app_id)
            .field("installation_id", &self.installation_id)
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct GithubConfig {
    pub gh_token: Option<SecretString>,
    pub apps: BTreeMap<String, AppCredentials>,
    pub base_uri: Option<String>,
    pub upload_uri: Option<String>,
}

impl fmt::Debug for GithubConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubConfig")
            .field("gh_token", &self.gh_token.as_ref().map(|_| "[REDACTED]"))
            .field("apps", &self.apps.keys().collect::<Vec<_>>())
            .field("base_uri", &self.base_uri)
            .field("upload_uri", &self.upload_uri)
            .finish()
    }
}

#[derive(Clone)]
pub struct GithubProvider {
    pub(crate) user: Option<Arc<Octocrab>>,
    pub(crate) apps: BTreeMap<String, Arc<Octocrab>>,
}

impl fmt::Debug for GithubProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubProvider")
            .field("user", &self.user.as_ref().map(|_| "configured"))
            .field("apps", &self.apps.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl GithubProvider {
    pub(crate) fn user(&self) -> Result<&Octocrab> {
        self.user
            .as_deref()
            .ok_or(ProviderError::MissingCredential {
                identity: "user".to_owned(),
                kind: "GH_TOKEN",
            })
    }

    pub(crate) fn app(&self, identity: &str) -> Result<&Octocrab> {
        self.apps
            .get(identity)
            .map(AsRef::as_ref)
            .ok_or_else(|| ProviderError::MissingCredential {
                identity: identity.to_owned(),
                kind: "named app",
            })
    }
}

pub async fn from_config(config: GithubConfig) -> Result<GithubProvider> {
    let user = config
        .gh_token
        .as_ref()
        .map(|token| build_user(token, &config))
        .transpose()?
        .map(Arc::new);

    let mut apps = BTreeMap::new();
    for (identity, credentials) in &config.apps {
        let key = EncodingKey::from_rsa_pem(credentials.private_key.expose_secret().as_bytes())
            .map_err(|_| ProviderError::Configuration {
                path: ".postel.toml".to_owned(),
                reason: format!("invalid RSA private key for app identity {identity}"),
            })?;
        let client = configured_builder(&config)?
            .app(credentials.app_id.into(), key)
            .build()
            .map_err(|error| external("construct app client", error))?
            .installation(credentials.installation_id.into())
            .map_err(|error| external("scope app installation", error))?;
        apps.insert(identity.clone(), Arc::new(client));
    }
    Ok(GithubProvider { user, apps })
}

pub async fn from_project(project_root: &Path) -> Result<GithubProvider> {
    from_project_with(&RealSystem, project_root).await
}

async fn from_project_with(system: &dyn System, project_root: &Path) -> Result<GithubProvider> {
    let path = project_root.join(".postel.toml");
    let contents =
        system
            .read_to_string(&path)
            .await
            .map_err(|error| ProviderError::Configuration {
                path: path.display().to_string(),
                reason: error.kind().to_string(),
            })?;
    let file: ProjectFile =
        toml::from_str(&contents).map_err(|error| ProviderError::Configuration {
            path: path.display().to_string(),
            reason: error.message().to_owned(),
        })?;
    from_config(file.provider.github.into()).await
}

fn build_user(token: &SecretString, config: &GithubConfig) -> Result<Octocrab> {
    configured_builder(config)?
        .personal_token(token.clone())
        .build()
        .map_err(|error| external("construct user client", error))
}

fn configured_builder(
    config: &GithubConfig,
) -> Result<OctocrabBuilder<NoSvc, DefaultOctocrabBuilderConfig, NoAuth, NotLayerReady>> {
    let mut builder = Octocrab::builder().add_retry_config(RetryConfig::HandleRateLimits {
        metrics: Arc::new(NoOpRateLimitMetrics),
        max_retries: 3,
        min_wait_seconds: 1,
    });
    if let Some(uri) = &config.base_uri {
        builder = builder
            .base_uri(uri)
            .map_err(|error| external("configure base URI", error))?;
    }
    if let Some(uri) = &config.upload_uri {
        builder = builder
            .upload_uri(uri)
            .map_err(|error| external("configure upload URI", error))?;
    }
    Ok(builder)
}

pub(crate) fn external(operation: &'static str, error: impl fmt::Display) -> ProviderError {
    ProviderError::External {
        provider: "github",
        operation,
        message: error.to_string(),
    }
}

pub(crate) fn read_error(
    operation: &'static str,
    entity: impl Into<String>,
    error: octocrab::Error,
) -> ProviderError {
    if matches!(
        &error,
        octocrab::Error::GitHub { source, .. } if source.status_code.as_u16() == 404
    ) {
        ProviderError::NotFound {
            entity: entity.into(),
        }
    } else {
        external(operation, error)
    }
}

#[derive(Deserialize)]
struct ProjectFile {
    provider: ProviderTable,
}

#[derive(Deserialize)]
struct ProviderTable {
    github: GithubTable,
}

#[derive(Deserialize)]
struct GithubTable {
    #[serde(rename = "GH_TOKEN")]
    gh_token: Option<String>,
    #[serde(default)]
    apps: BTreeMap<String, AppTable>,
    #[serde(rename = "BASE_URI")]
    base_uri: Option<String>,
    #[serde(rename = "UPLOAD_URI")]
    upload_uri: Option<String>,
}

#[derive(Deserialize)]
struct AppTable {
    #[serde(rename = "APP_ID")]
    app_id: u64,
    #[serde(rename = "INSTALLATION_ID")]
    installation_id: u64,
    #[serde(rename = "PRIVATE_KEY")]
    private_key: String,
}

impl From<GithubTable> for GithubConfig {
    fn from(table: GithubTable) -> Self {
        Self {
            gh_token: table.gh_token.map(Into::into),
            apps: table
                .apps
                .into_iter()
                .map(|(name, app)| {
                    (
                        name,
                        AppCredentials {
                            app_id: app.app_id,
                            installation_id: app.installation_id,
                            private_key: app.private_key.into(),
                        },
                    )
                })
                .collect(),
            base_uri: table.base_uri,
            upload_uri: table.upload_uri,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use postel_contracts::ProviderError;

    use super::{GithubConfig, GithubProvider, ProjectFile, from_config};
    use secrecy::SecretString;

    #[test]
    fn file_and_direct_forms_have_the_same_typed_shape() {
        let file: ProjectFile = toml::from_str(
            r#"
                [provider.github]
                GH_TOKEN = "user-secret"

                [provider.github.apps.automation]
                APP_ID = 12
                INSTALLATION_ID = 34
                PRIVATE_KEY = "app-secret"
            "#,
        )
        .expect("valid file");
        let from_file = GithubConfig::from(file.provider.github);
        let direct = GithubConfig {
            gh_token: Some(SecretString::from("user-secret")),
            apps: [(
                "automation".to_owned(),
                super::AppCredentials {
                    app_id: 12,
                    installation_id: 34,
                    private_key: SecretString::from("app-secret"),
                },
            )]
            .into_iter()
            .collect(),
            ..GithubConfig::default()
        };
        assert_eq!(format!("{from_file:?}"), format!("{direct:?}"));
    }

    #[test]
    fn debug_output_never_contains_credentials() {
        let config = GithubConfig {
            gh_token: Some(SecretString::from("sensitive-user-token")),
            ..GithubConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("sensitive-user-token"));
        assert!(debug.contains("REDACTED"));
    }

    #[tokio::test]
    async fn user_identity_cannot_substitute_for_a_named_app() {
        let provider = from_config(GithubConfig {
            gh_token: Some(SecretString::from("user-token")),
            ..GithubConfig::default()
        })
        .await
        .expect("construction is local");
        assert_eq!(
            provider.app("automation").expect_err("app is absent"),
            ProviderError::MissingCredential {
                identity: "automation".to_owned(),
                kind: "named app"
            }
        );
    }

    #[tokio::test]
    async fn named_app_identity_cannot_substitute_for_the_user() {
        let provider = GithubProvider {
            user: None,
            apps: BTreeMap::from([(
                "automation".to_owned(),
                Arc::new(octocrab::Octocrab::default()),
            )]),
        };
        assert_eq!(
            provider.user().expect_err("user is absent"),
            ProviderError::MissingCredential {
                identity: "user".to_owned(),
                kind: "GH_TOKEN"
            }
        );
    }
}
