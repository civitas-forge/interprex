//! Stateful in-memory implementations of all Postel domain contracts.

#![forbid(unsafe_code)]

mod code_reviews;
mod issues;
mod jobs;
mod releases;
mod repository;
mod state;

pub use state::FakeProvider;

#[cfg(test)]
mod tests;
