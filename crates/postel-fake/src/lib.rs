//! Stateful in-memory implementations of all five Postel domain contracts.
//!
//! See [`api`] for the test adapter. The fake records observable domain state,
//! so consumer tests remain valid when their internal call sequence changes.

#![forbid(unsafe_code)]

pub mod api;

pub use api::FakeProvider;
