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
        #[serde(try_from = "u64", into = "u64")]
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

        impl TryFrom<u64> for $name {
            type Error = crate::ModelError;

            fn try_from(value: u64) -> std::result::Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

pub mod code_hosting;
pub mod code_reviews;
pub mod error;
pub mod issues;
pub mod jobs;
pub mod provider;
pub mod releases;

pub use code_hosting::*;
pub use code_reviews::*;
pub use error::*;
pub use issues::*;
pub use jobs::*;
pub use provider::*;
pub use releases::*;

pub use CodeHostingProvider as CodeHostingDomain;
pub use CodeReviewsProvider as CodeReviewsDomain;
pub use IssuesProvider as TrackerDomain;
pub use JobsProvider as JobsDomain;
pub use ReleasesProvider as ReleasesDomain;
