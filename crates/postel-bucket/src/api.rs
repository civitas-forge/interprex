//! Public record-store interface.

use std::{fmt, sync::Arc};

use bytes::Bytes;
use futures_util::TryStreamExt;
use object_store::{ObjectStore, PutMode, PutOptions, gcp::GoogleCloudStorageBuilder, path::Path};
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum BucketError {
    #[error("record path must be relative, non-empty, and contain no empty, '.' or '..' segments")]
    InvalidPath,
    #[error("record already exists at {path}")]
    AlreadyExists { path: String },
    #[error("object store {operation} failed: {message}")]
    Store {
        operation: &'static str,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordPath(String);

impl RecordPath {
    pub fn new(value: impl Into<String>) -> Result<Self, BucketError> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('/')
            || value.ends_with('/')
            || value
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err(BucketError::InvalidPath);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn object_path(&self) -> Path {
        Path::from(self.0.clone())
    }
}

impl fmt::Display for RecordPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug)]
pub struct BucketClient {
    store: Arc<dyn ObjectStore>,
}

impl BucketClient {
    #[must_use]
    pub fn from_store(store: Arc<dyn ObjectStore>) -> Self {
        Self { store }
    }

    pub async fn create(&self, path: &RecordPath, content: Bytes) -> Result<(), BucketError> {
        let options = PutOptions {
            mode: PutMode::Create,
            ..PutOptions::default()
        };
        self.store
            .put_opts(&path.object_path(), content.into(), options)
            .await
            .map(drop)
            .map_err(|error| match error {
                object_store::Error::AlreadyExists { .. } => BucketError::AlreadyExists {
                    path: path.to_string(),
                },
                other => store_error("create", other),
            })
    }

    pub async fn get(&self, path: &RecordPath) -> Result<Bytes, BucketError> {
        self.store
            .get(&path.object_path())
            .await
            .map_err(|error| store_error("get", error))?
            .bytes()
            .await
            .map_err(|error| store_error("read", error))
    }

    pub async fn list(&self, prefix: &RecordPath) -> Result<Vec<RecordPath>, BucketError> {
        let mut entries = self
            .store
            .list(Some(&prefix.object_path()))
            .map_ok(|metadata| metadata.location.to_string())
            .try_collect::<Vec<_>>()
            .await
            .map_err(|error| store_error("list", error))?;
        entries.sort();
        entries
            .into_iter()
            .map(RecordPath::new)
            .collect::<Result<Vec<_>, _>>()
    }
}

pub fn from_gcs_env(bucket_name: &str) -> Result<BucketClient, BucketError> {
    let store = GoogleCloudStorageBuilder::from_env()
        .with_bucket_name(bucket_name)
        .build()
        .map_err(|error| store_error("construct Google Cloud Storage client", error))?;
    Ok(BucketClient::from_store(Arc::new(store)))
}

fn store_error(operation: &'static str, error: impl fmt::Display) -> BucketError {
    BucketError::Store {
        operation,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use object_store::memory::InMemory;

    use super::{BucketClient, BucketError, RecordPath};

    #[test]
    fn path_refuses_addresses_that_are_not_prefix_safe() {
        assert_eq!(RecordPath::new("../record"), Err(BucketError::InvalidPath));
        assert_eq!(RecordPath::new("/record"), Err(BucketError::InvalidPath));
        assert_eq!(RecordPath::new("record/"), Err(BucketError::InvalidPath));
    }

    #[tokio::test]
    async fn records_are_create_only_and_read_through_the_same_path() {
        let client = BucketClient::from_store(Arc::new(InMemory::new()));
        let path = RecordPath::new("sessions/2026/08/24/session.v1.json").expect("path");
        client
            .create(&path, Bytes::from_static(b"first"))
            .await
            .expect("create");
        assert_eq!(
            client.get(&path).await.expect("get"),
            Bytes::from_static(b"first")
        );
        assert_eq!(
            client.create(&path, Bytes::from_static(b"second")).await,
            Err(BucketError::AlreadyExists {
                path: path.to_string()
            })
        );
    }

    #[tokio::test]
    async fn listing_is_sorted_and_limited_to_the_requested_prefix() {
        let client = BucketClient::from_store(Arc::new(InMemory::new()));
        for path in [
            "sessions/2026/08/24/b.v1.json",
            "sessions/2026/08/24/a.v1.json",
            "sessions/2026/08/25/c.v1.json",
        ] {
            client
                .create(&RecordPath::new(path).expect("path"), Bytes::new())
                .await
                .expect("create");
        }
        let prefix = RecordPath::new("sessions/2026/08/24").expect("prefix");
        let listed = client.list(&prefix).await.expect("list");
        assert_eq!(
            listed.iter().map(RecordPath::as_str).collect::<Vec<_>>(),
            [
                "sessions/2026/08/24/a.v1.json",
                "sessions/2026/08/24/b.v1.json"
            ]
        );
    }
}
