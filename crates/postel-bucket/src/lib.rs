//! Create-only record storage over an injected object-store adapter.
//!
//! This module enforces the shared record discipline: one path names one
//! immutable object, creation never overwrites, listing is prefix-only, and the
//! public interface contains no vendor type. Retention is structural—there is
//! intentionally no delete operation. Path segment order remains the writer's
//! modeling decision, documented in `docs/contracts/records.lex`.

#![forbid(unsafe_code)]

pub mod api;

pub use api::{BucketClient, BucketError, RecordPath, from_gcs_env};
