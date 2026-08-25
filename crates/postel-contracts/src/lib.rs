//! Provider-neutral interfaces for the five development-platform domains.
//!
//! Domain traits are object-safe so a composition root can select providers at
//! runtime. They describe operations callers mean, never endpoint mechanics.
//! Cross-domain facts have one owner: for example check outcomes belong to pull
//! requests even when a jobs provider publishes them.

#![forbid(unsafe_code)]

pub mod api;

pub use api::*;
