//! Tauri command handlers bridging the front end to `bridgehop-core`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bridgehop_core::io::{export, ExportFormat};
use bridgehop_core::sources::{self, FetchResult, Selection};
use bridgehop_core::store::{Reliability, RunMeta, RunSummary, Store};
use bridgehop_core::{parse_bridge_lines, scan_bridges, ScanOptions, ScanResult};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
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

/// Delete all recorded scan history (runs, results, reliability), clearing the Library.
#[tauri::command]
pub async fn clear_history() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let mut store = Store::open().map_err(|e| e.to_string())?;
        store.clear().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Render bridge lines in the requested export format (plain / torrc / json).
#[tauri::command]
pub fn export_bridges(lines: Vec<String>, format: ExportFormat) -> String {
    export(&lines, format)
}

/// Prompt for a save location with a native dialog and write `contents` there. Returns the chosen
/// path, or `None` if the user cancelled.
#[cfg(desktop)]
#[tauri::command]
pub async fn save_text_file(name: String, contents: String) -> Result<Option<String>, String> {
    let chosen = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name(&name)
            .add_filter("Text", &["txt"])
            .add_filter("JSON", &["json"])
            .add_filter("torrc", &["torrc"])
            .add_filter("All files", &["*"])
            .save_file()
    })
    .await
    .map_err(|err| err.to_string())?;

    match chosen {
        Some(path) => {
            std::fs::write(&path, contents).map_err(|err| err.to_string())?;
            Ok(Some(path.display().to_string()))
        }
        None => Ok(None),
    }
}

/// Mobile has no native save dialog; the UI hides export there. Return a clear error if invoked.
#[cfg(not(desktop))]
#[tauri::command]
pub async fn save_text_file(_name: String, _contents: String) -> Result<Option<String>, String> {
    Err("Saving to a file isn't supported on this platform yet.".to_string())
}

/// Open a file picker, read the chosen file, and parse bridge lines from it (plain, torrc, or
/// JSON exported by BridgeHop). Returns the parsed lines, or `None` if the user cancelled.
#[cfg(desktop)]
#[tauri::command]
pub async fn import_bridges_file() -> Result<Option<Vec<String>>, String> {
    let chosen = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Bridge lists", &["txt", "json", "torrc"])
            .add_filter("All files", &["*"])
            .pick_file()
    })
    .await
    .map_err(|err| err.to_string())?;

    match chosen {
        Some(path) => {
            let content = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
            let lines = bridgehop_core::io::import(&content)
                .into_iter()
                .map(|b| b.raw)
                .collect();
            Ok(Some(lines))
        }
        None => Ok(None),
    }
}

/// Mobile has no native file picker; the UI hides import there. Return a clear error if invoked.
#[cfg(not(desktop))]
#[tauri::command]
pub async fn import_bridges_file() -> Result<Option<Vec<String>>, String> {
    Err("Importing from a file isn't supported on this platform yet.".to_string())
}

/// Render a bridge line (or any text) as an SVG QR code for sharing.
#[tauri::command]
pub fn qr_svg(text: String) -> Result<String, String> {
    bridgehop_core::io::qr_svg(&text).map_err(|err| err.to_string())
}

/// Encode a bridge line as a SlipNet `slipnet://` config URI for import into SlipNet.
#[tauri::command]
pub fn slipnet_uri(line: String) -> Result<String, String> {
    bridgehop_core::io::to_slipnet_uri(&line)
        .ok_or_else(|| "could not parse bridge line".to_string())
}

/// obfs4 / pluggable-transport availability for deep verify.
#[derive(serde::Serialize)]
pub struct DeepStatus {
    pub available: bool,
    pub pt_dir: String,
}

/// Whether an obfs4 client is installed, and where BridgeHop looks for PT binaries (desktop only).
#[cfg(desktop)]
#[tauri::command]
pub fn deep_status() -> DeepStatus {
    DeepStatus {
        available: bridgehop_core::scan::deep::obfs4_available(),
        pt_dir: bridgehop_core::scan::deep::pt_dir().display().to_string(),
    }
}

/// Deep verify spawns native PT clients, which mobile sandboxes forbid: always report unavailable.
#[cfg(not(desktop))]
#[tauri::command]
pub fn deep_status() -> DeepStatus {
    DeepStatus {
        available: false,
        pt_dir: String::new(),
    }
}

/// Open a URL (or path) with the OS default handler, via the opener plugin — no console flash on
/// Windows, and works on Android.
#[tauri::command]
pub fn open_external(app: AppHandle, target: String) -> Result<(), String> {
    app.opener()
        .open_url(target, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Create the pluggable-transport directory (if needed) and reveal it in the file manager.
#[cfg(desktop)]
#[tauri::command]
pub fn open_pt_dir(app: AppHandle) -> Result<(), String> {
    let dir = bridgehop_core::scan::deep::pt_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// No pluggable-transport directory on mobile (deep verify is desktop-only).
#[cfg(not(desktop))]
#[tauri::command]
pub fn open_pt_dir() -> Result<(), String> {
    Err("Deep verify isn't available on this platform.".to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
