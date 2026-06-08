//! Tauri command handlers bridging the front end to `bridgehop-core`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bridgehop_core::sources::{self, FetchResult, Selection};
use bridgehop_core::store::{Reliability, RunMeta, RunSummary, Store};
use bridgehop_core::{parse_bridge_lines, scan_bridges, ScanOptions, ScanResult};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Scan parameters sent from the UI.
#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub lines: Vec<String>,
    pub workers: usize,
    pub timeout_ms: u64,
    #[serde(default)]
    pub deep: bool,
    /// Optional label for where these bridges came from (e.g. a source URL or "manual").
    #[serde(default)]
    pub source: Option<String>,
}

/// Parse and scan the supplied bridge lines, streaming each result as a `scan-progress` event
/// and returning the full set when finished.
#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ScanRequest,
) -> Result<Vec<ScanResult>, String> {
    let bridges = parse_bridge_lines(request.lines.iter().map(String::as_str));
    let options = ScanOptions {
        workers: request.workers,
        timeout: Duration::from_millis(request.timeout_ms),
        deep: request.deep,
    };
    let started_unix = unix_now();
    let source = request.source.unwrap_or_else(|| "manual".to_string());
    let deep = request.deep;

    let cancel = CancellationToken::new();
    state.begin(cancel.clone());

    let (tx, mut rx) = mpsc::channel::<ScanResult>(64);
    let emitter = app.clone();
    let forward = tokio::spawn(async move {
        while let Some(result) = rx.recv().await {
            let _ = emitter.emit("scan-progress", &result);
        }
    });

    let results = scan_bridges(bridges, options, tx, cancel).await;
    let _ = forward.await;
    state.finish();

    // Persist the run (best-effort; never fail the scan because storage hiccuped).
    if !results.is_empty() {
        let to_store = results.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(mut store) = Store::open() {
                let meta = RunMeta {
                    started_unix,
                    source,
                    transport_filter: String::new(),
                    deep,
                };
                let _ = store.record_run(&meta, &to_store);
            }
        })
        .await;
    }

    let _ = app.emit("scan-done", results.len());
    Ok(results)
}

/// Cancel the in-flight scan, if any.
#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    state.cancel();
}

/// Fetch bridge lines from a source (collector mirror or built-in defaults).
#[tauri::command]
pub async fn fetch_bridges(selection: Selection) -> Result<FetchResult, String> {
    let client = sources::http_client();
    sources::fetch_with_cache(&selection, &client)
        .await
        .map_err(|err| err.to_string())
}

/// List recent scan runs (newest first).
#[tauri::command]
pub async fn list_runs(limit: usize) -> Result<Vec<RunSummary>, String> {
    tokio::task::spawn_blocking(move || {
        let store = Store::open().map_err(|e| e.to_string())?;
        store.list_runs(limit).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Per-bridge reliability leaderboard across all recorded scans.
#[tauri::command]
pub async fn reliability(limit: usize) -> Result<Vec<Reliability>, String> {
    tokio::task::spawn_blocking(move || {
        let store = Store::open().map_err(|e| e.to_string())?;
        store.reliability(limit).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
