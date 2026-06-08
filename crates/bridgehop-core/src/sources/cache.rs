//! On-disk cache of fetched bridge lists, with a stale fallback for offline / censored use.
//!
//! Best-effort: all I/O errors are swallowed (a missing or unreadable cache simply means no
//! fallback). Ported in spirit from OnionHop's bridge cache (7-day freshness + stale fallback).

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths;

const CACHE_FILE: &str = "source-cache.json";

#[derive(Default, Serialize, Deserialize)]
struct CacheStore {
    entries: HashMap<String, CacheEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// When these lines were fetched (Unix seconds); retained for diagnostics.
    pub updated_unix: u64,
    pub lines: Vec<String>,
}

/// Store fetched lines for a key.
pub fn put(key: &str, lines: &[String]) {
    let mut store = load();
    store.entries.insert(
        key.to_string(),
        CacheEntry {
            updated_unix: now_unix(),
            lines: lines.to_vec(),
        },
    );
    let _ = save(&store);
}

/// Retrieve a cached entry for a key, if present.
pub fn get(key: &str) -> Option<CacheEntry> {
    load().entries.get(key).cloned()
}

fn load() -> CacheStore {
    let path = paths::data_dir().join(CACHE_FILE);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(store: &CacheStore) -> std::io::Result<()> {
    let dir = paths::ensure_data_dir()?;
    let json = serde_json::to_string(store).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(CACHE_FILE), json)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
