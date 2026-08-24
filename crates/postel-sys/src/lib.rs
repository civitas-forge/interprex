//! Injectable host access used by Postel's edge adapters.
//!
//! Only effects that Postel itself performs belong here. Network behavior stays
//! in provider crates, while filesystem reads and waiting use this small seam so
//! parsing and policy tests need neither disk nor time.

#![forbid(unsafe_code)]

pub mod api;

pub use api::{RealSystem, System};
