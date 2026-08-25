use std::{collections::BTreeMap, fmt, io, path::Path, sync::Arc};

use jsonwebtoken::EncodingKey;
use octocrab::{
    DefaultOctocrabBuilderConfig, NoAuth, NoSvc, NotLayerReady, Octocrab, OctocrabBuilder,
    service::middleware::retry::{NoOpRateLimitMetrics, RetryConfig},
};
use postel::{ConfigurationSource, ProviderError, Result};
use secrecy::ExposeSecret;

use crate::config::{GithubConfig, parse_project_config};

#[derive(Clone)]
pub struct GithubProvider {
    pub(crate) user: Option<Arc<Octocrab>>,
    pub(crate) streaming_user: Option<Arc<Octocrab>>,
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

    pub(crate) fn streaming_user(&self) -> Result<&Octocrab> {
        self.streaming_user
            .as_deref()
            .ok_or(ProviderError::MissingCredential {
                identity: "user".to_owned(),
                kind: "GH_TOKEN",
            })
    }
}

pub fn from_config(config: GithubConfig) -> Result<GithubProvider> {
    from_config_with_source(config, ConfigurationSource::Direct)
}

fn from_config_with_source(
    config: GithubConfig,
    source: ConfigurationSource,
) -> Result<GithubProvider> {
    let user = config
        .gh_token
        .as_ref()
        .map(|token| build_user(token, &config))
        .transpose()?
        .map(Arc::new);
    let streaming_user = config
        .gh_token
        .as_ref()
        .map(|token| build_streaming_user(token, &config))
        .transpose()?
        .map(Arc::new);

    let mut apps = BTreeMap::new();
    for (identity, credentials) in &config.apps {
        let key = EncodingKey::from_rsa_pem(credentials.private_key.expose_secret().as_bytes())
            .map_err(|_| ProviderError::Configuration {
                origin: source.clone(),
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
    Ok(GithubProvider {
        user,
        streaming_user,
        apps,
    })
}

pub async fn from_project(project_root: &Path) -> Result<GithubProvider> {
    let path = project_root.join(".postel.toml");
    from_project_read(project_root, tokio::fs::read_to_string(path).await)
}

fn from_project_read(project_root: &Path, contents: io::Result<String>) -> Result<GithubProvider> {
    let path = project_root.join(".postel.toml");
    let source = ConfigurationSource::File(path.display().to_string());
    let contents = contents.map_err(|error| ProviderError::Configuration {
        origin: source.clone(),
        reason: error.kind().to_string(),
    })?;
    let config = parse_project_config(&contents, &source)?;
    from_config_with_source(config, source)
}

fn build_user(token: &secrecy::SecretString, config: &GithubConfig) -> Result<Octocrab> {
    configured_builder(config)?
        .personal_token(token.clone())
        .build()
        .map_err(|error| external("construct user client", error))
}

fn build_streaming_user(token: &secrecy::SecretString, config: &GithubConfig) -> Result<Octocrab> {
    base_builder(config)?
        .personal_token(token.clone())
        .build()
        .map_err(|error| external("construct streaming user client", error))
}

fn configured_builder(
    config: &GithubConfig,
) -> Result<OctocrabBuilder<NoSvc, DefaultOctocrabBuilderConfig, NoAuth, NotLayerReady>> {
    Ok(
        base_builder(config)?.add_retry_config(RetryConfig::HandleRateLimits {
            metrics: Arc::new(NoOpRateLimitMetrics),
            max_retries: 3,
            min_wait_seconds: 1,
        }),
    )
}

fn base_builder(
    config: &GithubConfig,
) -> Result<OctocrabBuilder<NoSvc, DefaultOctocrabBuilderConfig, NoAuth, NotLayerReady>> {
    let mut builder = Octocrab::builder();
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

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use postel::{ConfigurationSource, ProviderError};
    use secrecy::SecretString;

    use super::{GithubProvider, from_config, from_project, from_project_read};
    use crate::{AppCredentials, GithubConfig};

    struct TempProject(PathBuf);

    impl TempProject {
        fn new(contents: Option<&str>) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "postel-github-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).expect("create temporary project");
            if let Some(contents) = contents {
                std::fs::write(path.join(".postel.toml"), contents)
                    .expect("write temporary project config");
            }
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove temporary project");
        }
    }

    #[test]
    fn invalid_direct_app_key_reports_direct_construction() {
        let error = from_config(GithubConfig {
            apps: BTreeMap::from([(
                "automation".to_owned(),
                AppCredentials {
                    app_id: 12,
                    installation_id: 34,
                    private_key: SecretString::from("not-an-rsa-key"),
                },
            )]),
            ..GithubConfig::default()
        })
        .expect_err("invalid key");
        assert!(error.to_string().contains("direct"));
    }

    #[tokio::test]
    async fn project_file_constructs_the_same_identity_as_direct_input() {
        let project = TempProject::new(Some(
            r#"
                [provider.github]
                GH_TOKEN = "user-secret"
            "#,
        ));
        let from_project = from_project(project.path())
            .await
            .expect("project provider");
        let direct = from_config(GithubConfig {
            gh_token: Some(SecretString::from("user-secret")),
            ..GithubConfig::default()
        })
        .expect("direct provider");

        assert_eq!(format!("{from_project:?}"), format!("{direct:?}"));
        assert!(from_project.user().is_ok());
        assert!(from_project.streaming_user().is_ok());
    }

    #[tokio::test]
    async fn project_app_error_reports_the_file_that_supplied_it() {
        let project = TempProject::new(Some(
            r#"
                [provider.github]

                [provider.github.apps.automation]
                APP_ID = 12
                INSTALLATION_ID = 34
                PRIVATE_KEY = "not-an-rsa-key"
            "#,
        ));
        let error = from_project(project.path()).await.expect_err("invalid key");
        assert!(error.to_string().contains(&format!(
            "file {}",
            project.path().join(".postel.toml").display()
        )));
    }

    #[tokio::test]
    async fn missing_project_file_returns_a_structured_configuration_error() {
        let project = TempProject::new(None);
        let error = from_project(project.path())
            .await
            .expect_err("missing file");
        assert_eq!(
            error,
            ProviderError::Configuration {
                origin: ConfigurationSource::File(
                    project.path().join(".postel.toml").display().to_string()
                ),
                reason: std::io::ErrorKind::NotFound.to_string(),
            }
        );
    }

    #[test]
    fn unreadable_project_file_returns_a_structured_configuration_error() {
        let project_root = Path::new("/workspace/project");
        let error = from_project_read(
            project_root,
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        )
        .expect_err("unreadable file");
        assert_eq!(
            error,
            ProviderError::Configuration {
                origin: ConfigurationSource::File(
                    project_root.join(".postel.toml").display().to_string()
                ),
                reason: std::io::ErrorKind::PermissionDenied.to_string(),
            }
        );
    }

    #[tokio::test]
    async fn user_and_app_identities_remain_distinct() {
        let app_only = GithubProvider {
            user: None,
            streaming_user: None,
            apps: BTreeMap::from([(
                "automation".to_owned(),
                Arc::new(octocrab::Octocrab::default()),
            )]),
        };
        assert_eq!(
            app_only.user().expect_err("user is absent"),
            ProviderError::MissingCredential {
                identity: "user".to_owned(),
                kind: "GH_TOKEN"
            }
        );

        let user = Arc::new(octocrab::Octocrab::default());
        let user_only = GithubProvider {
            user: Some(Arc::clone(&user)),
            streaming_user: Some(user),
            apps: BTreeMap::new(),
        };
        assert_eq!(
            user_only
                .app("automation")
                .expect_err("named app is absent"),
            ProviderError::MissingCredential {
                identity: "automation".to_owned(),
                kind: "named app"
            }
        );
    }
}
