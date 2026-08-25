use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Repository, Result};

platform_number!(ReleaseId);
platform_number!(AssetId);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Release {
    pub id: ReleaseId,
    pub tag: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseAsset {
    pub id: AssetId,
    pub name: String,
    pub label: Option<String>,
    pub size: u64,
    pub download_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NewRelease {
    pub tag: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub target: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct AssetStreamError {
    message: String,
}

impl AssetStreamError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type AssetStreamItem = std::result::Result<Bytes, AssetStreamError>;
pub type AssetStream = Pin<Box<dyn Stream<Item = AssetStreamItem> + Send + Sync + 'static>>;

/// A one-shot upload body with the exact byte length required by the provider.
///
/// Providers consume the stream once and do not retry a partially sent body.
pub struct AssetUpload {
    content_length: u64,
    chunks: AssetStream,
}

impl AssetUpload {
    pub fn new<S>(content_length: u64, chunks: S) -> Self
    where
        S: Stream<Item = AssetStreamItem> + Send + Sync + 'static,
    {
        Self {
            content_length,
            chunks: Box::pin(chunks),
        }
    }

    #[must_use]
    pub fn into_parts(self) -> (u64, AssetStream) {
        (self.content_length, self.chunks)
    }
}

#[async_trait]
pub trait ReleasesProvider: Send + Sync {
    async fn release_by_tag(&self, repository: &Repository, tag: &str) -> Result<Release>;
    async fn create_release(
        &self,
        repository: &Repository,
        release: &NewRelease,
    ) -> Result<Release>;
    async fn upload_asset(
        &self,
        repository: &Repository,
        release_id: ReleaseId,
        name: &str,
        label: Option<&str>,
        upload: AssetUpload,
    ) -> Result<ReleaseAsset>;
    /// Opens a chunk stream without buffering the complete asset.
    async fn download_asset(
        &self,
        repository: &Repository,
        asset_id: AssetId,
    ) -> Result<AssetStream>;
}
