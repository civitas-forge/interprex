//! Release and asset operations owned by the releases domain.
//!
//! Release metadata is normalized into the shared model. Downloads return
//! Octocrab's response chunks directly. Uploads accept a declared length and a
//! one-shot stream, use GitHub's upload host, and deliberately bypass request
//! retries: a partially consumed stream cannot be replayed safely. A caller may
//! retry the whole operation only by constructing a fresh stream.

use async_trait::async_trait;
use futures_util::TryStreamExt;
use http::{Request, header};
use http_body::Frame;
use http_body_util::StreamBody;
use interprex::{
    AssetId, AssetStream, AssetStreamError, AssetUpload, NewRelease, ProviderError, Release,
    ReleaseAsset, ReleaseId, ReleasesProvider, Repository, Result,
};
use octocrab::{FromResponse, OctoBody};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, client::external};

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
impl ReleasesProvider for GithubProvider {
    async fn release_by_tag(&self, repository: &Repository, tag: &str) -> Result<Release> {
        let response: GithubRelease = self
            .user()?
            .get(
                format!("/repos/{repository}/releases/tags/{tag}"),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::client::read_error(
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
        upload: AssetUpload,
    ) -> Result<ReleaseAsset> {
        let handler = self.user()?.repos(repository.owner(), repository.name());
        let releases = handler.releases();
        let release = releases
            .get(release_id.get())
            .await
            .map_err(|error| external("read release for asset upload", error))?;
        let mut upload_url = format!(
            "{}?name={}",
            release.upload_url.replace("{?name,label}", ""),
            utf8_percent_encode(name, NON_ALPHANUMERIC)
        );
        if let Some(label) = label {
            upload_url.push_str("&label=");
            upload_url.push_str(&utf8_percent_encode(label, NON_ALPHANUMERIC).to_string());
        }
        let (content_length, chunks) = upload.into_parts();
        let frames = chunks.map_ok(Frame::data);
        let request = Request::builder()
            .method(http::Method::POST)
            .uri(upload_url)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, content_length)
            .body(OctoBody::new(StreamBody::new(frames)))
            .map_err(|error| external("construct release asset upload", error))?;
        let response = self
            .streaming_user()?
            .execute(request)
            .await
            .map_err(|error| external("upload release asset", error))?;
        let response = octocrab::map_github_error(response)
            .await
            .map_err(|error| external("upload release asset", error))?;
        let response = GithubAsset::from_response(response)
            .await
            .map_err(|error| external("decode release asset upload", error))?;
        normalize_asset(response)
    }

    async fn download_asset(
        &self,
        repository: &Repository,
        asset_id: AssetId,
    ) -> Result<AssetStream> {
        let handler = self.user()?.repos(repository.owner(), repository.name());
        let stream = handler
            .release_assets()
            .stream(asset_id.get())
            .await
            .map_err(|error| external("open release asset stream", error))?;
        Ok(Box::pin(
            stream.map_err(|error| AssetStreamError::new(error.to_string())),
        ))
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
        assert_eq!(release.assets[0].name, "interprex.tar.gz");
    }
}
