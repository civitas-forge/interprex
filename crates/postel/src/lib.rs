//! Provider-neutral models and interfaces for development-platform domains.

#![forbid(unsafe_code)]

macro_rules! platform_number {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            serde::Deserialize,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            serde::Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub fn new(value: u64) -> std::result::Result<Self, crate::ModelError> {
                (value > 0)
                    .then_some(Self(value))
                    .ok_or(crate::ModelError::InvalidNumber)
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

pub mod error;
pub mod issues;
pub mod jobs;
pub mod provider;
pub mod pull_requests;
pub mod releases;
pub mod repository;

pub use error::*;
pub use issues::*;
pub use jobs::*;
pub use provider::*;
pub use pull_requests::*;
pub use releases::*;
pub use repository::*;

pub use IssuesProvider as TrackerDomain;
pub use JobsProvider as JobsDomain;
pub use PullRequestsProvider as PrDomain;
pub use ReleasesProvider as ReleasesDomain;
pub use RepositoryProvider as RepoDomain;
