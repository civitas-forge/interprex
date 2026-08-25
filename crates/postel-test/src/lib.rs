//! Stateful in-memory implementations of all Postel domain contracts.

#![forbid(unsafe_code)]

mod issues;
mod jobs;
mod pull_requests;
mod releases;
mod repository;
mod state;

pub use state::FakeProvider;

#[cfg(test)]
mod tests;
