//! BridgeHop desktop shell.
//!
//! A thin Tauri layer over `bridgehop-core`: it exposes scan commands to the front end and
//! forwards streamed results as `scan-progress` / `scan-done` events.

mod commands;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::cancel_scan,
            commands::fetch_bridges,
            commands::list_runs,
            commands::reliability,
            commands::export_bridges,
            commands::qr_svg
        ])
        .run(tauri::generate_context!())
        .expect("error while running BridgeHop");
}
