//! Injectable host access used by Postel's edge adapters.

#![forbid(unsafe_code)]

use std::{io, path::Path};

use async_trait::async_trait;

#[async_trait]
pub trait System: Send + Sync {
    async fn read_to_string(&self, path: &Path) -> io::Result<String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RealSystem;

#[async_trait]
impl System for RealSystem {
    async fn read_to_string(&self, path: &Path) -> io::Result<String> {
        tokio::fs::read_to_string(path).await
    }
}
