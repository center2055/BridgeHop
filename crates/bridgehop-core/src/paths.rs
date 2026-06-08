//! Filesystem locations for BridgeHop's data.
//!
//! Everything lives under one per-OS application-data folder named `BridgeHop`
//! (e.g. `%LOCALAPPDATA%\BridgeHop` on Windows, `~/.local/share/BridgeHop` on Linux,
//! `~/Library/Application Support/BridgeHop` on macOS), matching the "Hop" family convention.

use std::path::PathBuf;

/// The BridgeHop data directory (not guaranteed to exist yet).
pub fn data_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("BridgeHop"))
        .unwrap_or_else(|| PathBuf::from("BridgeHop-data"))
}

/// The BridgeHop data directory, created if necessary.
pub fn ensure_data_dir() -> std::io::Result<PathBuf> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
