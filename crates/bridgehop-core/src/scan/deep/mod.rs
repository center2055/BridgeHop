//! Tier T3: deep verification by launching real pluggable-transport clients.
//!
//! Desktop-only and gated behind the `deep-verify` feature.

mod pt;
mod socks;

pub use pt::{deep_verify, obfs4_available, pt_dir};
