//! GitHub provider for Postel's five domain contracts.
//!
//! The provider owns authentication, rate-aware retries, response
//! normalization, GraphQL documents, secret encryption, and release transport.
//! Domain callers see only `postel-contracts`. Construction parses credentials
//! but performs no request; missing identities fail only when an operation
//! requires them.
//!
//! Collection reads follow GitHub's REST links or GraphQL cursors until the
//! provider reports no next page. Missing continuation metadata is an error;
//! callers never receive a silently truncated collection.

#![forbid(unsafe_code)]

pub mod api;
mod jobs;
mod pr;
mod releases;
mod repo;
mod tracker;

pub use api::{AppCredentials, GithubConfig, GithubProvider, from_config, from_project};
