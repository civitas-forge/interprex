//! Release and asset operations owned by the releases domain.
//!
//! Release metadata is normalized into the shared model. Asset traffic uses
//! Octocrab's dedicated upload and download routes rather than generic JSON
//! calls, keeping upload-host selection, authentication, and response handling
//! in the one system client.

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures_util::TryStreamExt;
use postel_contracts::{ProviderError, ReleasesDomain, Result};
use postel_model::{AssetId, NewRelease, Release, ReleaseAsset, ReleaseId, Repository};
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, api::external};

#[derive(Deserialize)]
struct GithubRelease {
    id: u64,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    id: u64,
    name: String,
    label: Option<String>,
    size: i64,
    browser_download_url: String,
}

fn normalize_asset(value: GithubAsset) -> Result<ReleaseAsset> {
    Ok(ReleaseAsset {
        id: AssetId::new(value.id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize release asset",
            message: error.to_string(),
        })?,
        name: value.name,
        label: value.label,
        size: u64::try_from(value.size).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize release asset",
            message: error.to_string(),
        })?,
        download_url: value.browser_download_url,
    })
}

fn normalize_release(value: GithubRelease) -> Result<Release> {
    Ok(Release {
        id: ReleaseId::new(value.id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize release",
            message: error.to_string(),
        })?,
        tag: value.tag_name,
        name: value.name,
        body: value.body,
        draft: value.draft,
        prerelease: value.prerelease,
        assets: value
            .assets
            .into_iter()
            .map(normalize_asset)
            .collect::<Result<_>>()?,
    })
}

#[async_trait]
impl ReleasesDomain for GithubProvider {
    async fn release_by_tag(&self, repository: &Repository, tag: &str) -> Result<Release> {
        let response: GithubRelease = self
            .user()?
            .get(
                format!("/repos/{repository}/releases/tags/{tag}"),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::api::read_error(
                    "read release by tag",
                    format!("release {tag} in {repository}"),
                    error,
                )
            })?;
        normalize_release(response)
    }

    async fn create_release(
        &self,
        repository: &Repository,
        release: &NewRelease,
    ) -> Result<Release> {
        let response: GithubRelease = self
            .user()?
            .post(
                format!("/repos/{repository}/releases"),
                Some(&json!({
                    "tag_name": release.tag,
                    "name": release.name,
                    "body": release.body,
                    "target_commitish": release.target,
                    "draft": release.draft,
                    "prerelease": release.prerelease,
                })),
            )
            .await
            .map_err(|error| external("create release", error))?;
        normalize_release(response)
    }

    async fn upload_asset(
        &self,
        repository: &Repository,
        release_id: ReleaseId,
        name: &str,
        label: Option<&str>,
        content: Bytes,
    ) -> Result<ReleaseAsset> {
        let handler = self.user()?.repos(repository.owner(), repository.name());
        let releases = handler.releases();
        let mut upload = releases.upload_asset(release_id.get(), &name, content);
        if let Some(label) = label {
            upload = upload.label(label);
        }
        let response = upload
            .send()
            .await
            .map_err(|error| external("upload release asset", error))?;
        normalize_asset(GithubAsset {
            id: response.id.0,
            name: response.name,
            label: response.label,
            size: response.size,
            browser_download_url: response.browser_download_url.to_string(),
        })
    }

    async fn download_asset(&self, repository: &Repository, asset_id: AssetId) -> Result<Bytes> {
        let handler = self.user()?.repos(repository.owner(), repository.name());
        let mut stream = handler
            .release_assets()
            .stream(asset_id.get())
            .await
            .map_err(|error| external("open release asset stream", error))?;
        let mut content = BytesMut::new();
        while let Some(chunk) = stream
            .try_next()
            .await
            .map_err(|error| external("read release asset stream", error))?
        {
            content.extend_from_slice(&chunk);
        }
        Ok(content.freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::{GithubRelease, normalize_release};

    #[test]
    fn release_fixture_normalizes_assets() {
        let response: GithubRelease =
            serde_json::from_str(include_str!("../tests/fixtures/release.json")).expect("fixture");
        let release = normalize_release(response).expect("normalizes");
        assert_eq!(release.tag, "v0.1.0");
        assert_eq!(release.assets[0].name, "postel.tar.gz");
    }
}
