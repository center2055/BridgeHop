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
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|_app| {
            // On mobile the per-OS data location isn't discoverable via `directories`, so hand the
            // core engine the app's sandboxed data dir for its SQLite store and source cache.
            #[cfg(mobile)]
            {
                use tauri::Manager;
                if let Ok(dir) = _app.path().app_data_dir() {
                    bridgehop_core::paths::set_data_dir(dir);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::cancel_scan,
            commands::fetch_bridges,
            commands::list_runs,
            commands::reliability,
            commands::export_bridges,
            commands::save_text_file,
            commands::import_bridges_file,
            commands::qr_svg,
            commands::deep_status,
            commands::open_external,
            commands::open_pt_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running BridgeHop");
}
