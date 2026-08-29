//! GitHub provider for Interprex's development-platform contracts.

#![forbid(unsafe_code)]

mod client;
mod code_hosting;
mod code_reviews;
mod config;
mod issues;
mod jobs;
mod releases;
mod source_code_configuration;

pub use client::{GithubProvider, from_config, from_project};
pub use config::{AppCredentials, GithubConfig};
pub use source_code_configuration::{
    GithubRefNameCondition, GithubRuleset, GithubRulesetBypassActor, GithubRulesetConditions,
    GithubRulesetRule,
};
