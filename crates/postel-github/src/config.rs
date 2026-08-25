use std::{collections::BTreeMap, fmt};

use postel::{ConfigurationSource, ProviderError, Result};
use secrecy::SecretString;
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

pub(crate) fn parse_project_config(
    contents: &str,
    source: &ConfigurationSource,
) -> Result<GithubConfig> {
    let file: ProjectFile =
        toml::from_str(contents).map_err(|error| ProviderError::Configuration {
            origin: source.clone(),
            reason: error.message().to_owned(),
        })?;
    Ok(file.provider.github.into())
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
    use postel::{ConfigurationSource, ProviderError};
    use secrecy::{ExposeSecret, SecretString};

    use super::{AppCredentials, GithubConfig, parse_project_config};

    #[test]
    fn file_and_direct_forms_preserve_the_same_credentials() {
        let from_file = parse_project_config(
            r#"
                [provider.github]
                GH_TOKEN = "user-secret"

                [provider.github.apps.automation]
                APP_ID = 12
                INSTALLATION_ID = 34
                PRIVATE_KEY = "app-secret"
            "#,
            &ConfigurationSource::File(".postel.toml".to_owned()),
        )
        .expect("valid file");
        let direct = GithubConfig {
            gh_token: Some(SecretString::from("user-secret")),
            apps: [(
                "automation".to_owned(),
                AppCredentials {
                    app_id: 12,
                    installation_id: 34,
                    private_key: SecretString::from("app-secret"),
                },
            )]
            .into_iter()
            .collect(),
            ..GithubConfig::default()
        };

        assert_eq!(
            from_file.gh_token.as_ref().map(ExposeSecret::expose_secret),
            direct.gh_token.as_ref().map(ExposeSecret::expose_secret)
        );
        let file_app = &from_file.apps["automation"];
        let direct_app = &direct.apps["automation"];
        assert_eq!(file_app.app_id, direct_app.app_id);
        assert_eq!(file_app.installation_id, direct_app.installation_id);
        assert_eq!(
            file_app.private_key.expose_secret(),
            direct_app.private_key.expose_secret()
        );
    }

    #[test]
    fn malformed_project_config_retains_its_source() {
        let source = ConfigurationSource::File("/workspace/project/.postel.toml".to_owned());
        let error = parse_project_config("[provider.github\nGH_TOKEN = broken", &source)
            .expect_err("malformed file");
        match error {
            ProviderError::Configuration { origin, reason } => {
                assert_eq!(origin, source);
                assert!(!reason.is_empty());
            }
            other => panic!("unexpected error: {other:?}"),
        }
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
}
