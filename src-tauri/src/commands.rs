//! Tauri command handlers bridging the front end to `bridgehop-core`.

use std::time::Duration;

use bridgehop_core::sources::{self, FetchResult, Selection};
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
