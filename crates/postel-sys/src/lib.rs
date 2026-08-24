//! Injectable host access used by Postel's edge adapters.
//!
//! Only effects that Postel itself performs belong here. Network behavior stays
//! in provider crates, while filesystem reads use this small seam so parsing
//! and policy tests need no disk.

#![forbid(unsafe_code)]

pub mod api;

pub use api::{RealSystem, System};
