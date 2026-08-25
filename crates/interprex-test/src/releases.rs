use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use interprex::{
    AssetId, AssetStream, AssetStreamError, AssetUpload, NewRelease, ProviderError, Release,
    ReleaseAsset, ReleaseId, ReleasesProvider, Repository, Result,
};

use crate::state::{FakeProvider, missing};

#[async_trait]
impl ReleasesProvider for FakeProvider {
    async fn release_by_tag(&self, repository: &Repository, tag: &str) -> Result<Release> {
        self.state
            .read()
            .await
            .releases
            .get(&(repository.clone(), tag.to_owned()))
            .cloned()
            .ok_or_else(|| missing(format!("release {tag} in {repository}")))
    }

    async fn create_release(
        &self,
        repository: &Repository,
        release: &NewRelease,
    ) -> Result<Release> {
        let mut state = self.state.write().await;
        state.next_release_id += 1;
        let created = Release {
            id: ReleaseId::new(state.next_release_id).expect("increment starts at one"),
            tag: release.tag.clone(),
            name: release.name.clone(),
            body: release.body.clone(),
            draft: release.draft,
            prerelease: release.prerelease,
            assets: Vec::new(),
        };
        state
            .releases
            .insert((repository.clone(), created.tag.clone()), created.clone());
        Ok(created)
    }

    async fn upload_asset(
        &self,
        repository: &Repository,
        release_id: ReleaseId,
        name: &str,
        label: Option<&str>,
        upload: AssetUpload,
    ) -> Result<ReleaseAsset> {
        let (content_length, chunks) = upload.into_parts();
        let chunks: Vec<Bytes> =
            chunks
                .try_collect()
                .await
                .map_err(|error| ProviderError::External {
                    provider: "fake",
                    operation: "read release asset upload",
                    message: error.to_string(),
                })?;
        let actual_length = chunks.iter().try_fold(0_u64, |length, chunk| {
            length.checked_add(chunk.len() as u64)
        });
        if actual_length != Some(content_length) {
            return Err(ProviderError::Refused {
                provider: "fake",
                fact: format!(
                    "asset upload declared {content_length} bytes but yielded {}",
                    actual_length.map_or_else(
                        || "an overflowing length".to_owned(),
                        |value| value.to_string()
                    )
                ),
            });
        }
        let mut state = self.state.write().await;
        state.next_asset_id += 1;
        let asset = ReleaseAsset {
            id: AssetId::new(state.next_asset_id).expect("increment starts at one"),
            name: name.to_owned(),
            label: label.map(str::to_owned),
            size: content_length,
            download_url: format!("memory://{}/{}", repository, state.next_asset_id),
        };
        let release = state
            .releases
            .values_mut()
            .find(|release| release.id == release_id)
            .ok_or_else(|| missing(format!("release {release_id:?}")))?;
        release.assets.push(asset.clone());
        state.assets.insert((repository.clone(), asset.id), chunks);
        Ok(asset)
    }

    async fn download_asset(
        &self,
        repository: &Repository,
        asset_id: AssetId,
    ) -> Result<AssetStream> {
        let chunks = self
            .state
            .read()
            .await
            .assets
            .get(&(repository.clone(), asset_id))
            .cloned()
            .ok_or_else(|| missing(format!("asset {asset_id:?} in {repository}")))?;
        Ok(Box::pin(stream::iter(
            chunks.into_iter().map(Ok::<Bytes, AssetStreamError>),
        )))
    }
}
