//! Transport contract tests run Octocrab against a one-request local server.
//!
//! These tests do not duplicate Octocrab's HTTP implementation. They prove the
//! Interprex-owned endpoint choice, parameters, identity, and response
//! normalization for each domain before a request would reach GitHub.

#[path = "http_contract/code_hosting.rs"]
mod code_hosting;
#[path = "http_contract/code_reviews.rs"]
mod code_reviews;
#[path = "http_contract/http_fixture.rs"]
mod http_fixture;
#[path = "http_contract/issues.rs"]
mod issues;
#[path = "http_contract/jobs.rs"]
mod jobs;
#[path = "http_contract/releases.rs"]
mod releases;
