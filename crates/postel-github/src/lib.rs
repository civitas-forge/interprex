//! GitHub provider for Postel's development-platform contracts.

#![forbid(unsafe_code)]

mod client;
mod code_reviews;
mod config;
mod issues;
mod jobs;
mod releases;
mod repository;

pub use client::{GithubProvider, from_config, from_project};
pub use config::{AppCredentials, GithubConfig};
