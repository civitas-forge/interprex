//! Stateful in-memory implementations of all Interprex domain contracts.

#![forbid(unsafe_code)]

mod code_hosting;
mod code_reviews;
mod issues;
mod jobs;
mod releases;
mod state;

pub use state::FakeProvider;

#[cfg(test)]
mod tests;
