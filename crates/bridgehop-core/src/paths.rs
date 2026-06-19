//! Filesystem locations for BridgeHop's data.
//!
//! Everything lives under one per-OS application-data folder named `BridgeHop`
//! (e.g. `%LOCALAPPDATA%\BridgeHop` on Windows, `~/.local/share/BridgeHop` on Linux,
//! `~/Library/Application Support/BridgeHop` on macOS), matching the "Hop" family convention.

use std::path::PathBuf;
use std::sync::OnceLock;

/// An explicit data directory set by the host application, used where the per-OS default isn't
/// discoverable or writable (notably Android, where the app supplies its sandboxed data path).
static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Override the data directory. Call once at startup, before any store/cache access. Ignored if
/// the directory has already been resolved or set. Desktop and the CLI normally leave this unset
/// and fall back to the per-OS location below.
pub fn set_data_dir(path: PathBuf) {
    let _ = DATA_DIR_OVERRIDE.set(path);
}

/// The BridgeHop data directory (not guaranteed to exist yet).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = DATA_DIR_OVERRIDE.get() {
        return dir.clone();
    }
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
