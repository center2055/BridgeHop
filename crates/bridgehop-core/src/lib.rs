//! BridgeHop core engine.
//!
//! Everything BridgeHop does — parsing bridge lines, probing reachability, fetching
//! bridge sources, persistence, geo lookups and import/export — lives here. The Tauri
//! desktop app and the CLI are thin shells over this crate so they always share one
//! implementation.

pub mod error;
pub mod model;
pub mod parse;
pub mod scan;

pub use error::{Error, Result};
pub use model::{Bridge, Endpoint, Reachability, ScanResult, Transport, SLOW_THRESHOLD_MS};
pub use parse::{parse_bridge_line, parse_bridge_lines};
pub use scan::{probe_bridge, scan as scan_bridges, ScanOptions};
