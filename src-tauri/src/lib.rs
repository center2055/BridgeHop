//! BridgeHop desktop shell.
//!
//! A thin Tauri layer over `bridgehop-core`: it exposes scan commands to the front end and
//! forwards streamed results as `scan-progress` / `scan-done` events.

mod commands;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // On some Linux GPU/driver/VM combos (and in the AppImage) WebKitGTK's GPU compositing path
    // can't create an EGL display ("EGL_BAD_PARAMETER. Aborting..."), so the web process dies and
    // the window is blank/white. Disabling compositing forces software rendering (no EGL needed),
    // which is the canonical fix; also disable the DMABUF renderer. Both are skipped if the user
    // set them, so a working GPU setup can re-enable acceleration. The app is light enough that
    // software rendering is imperceptible.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    // Windows: the self-contained portable build ships a fixed-version WebView2 runtime in a
    // `WebView2Runtime` folder next to the executable. If one is present, point WebView2 at it so
    // the app runs without a system-installed runtime. This must happen before the webview is
    // created (WebView2 reads the variable when the environment is built). A no-op for the normal
    // builds, which ship no such folder and use the system (Evergreen) runtime.
    #[cfg(target_os = "windows")]
    {
        if std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER").is_none() {
            if let Some(runtime) = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("WebView2Runtime")))
            {
                if runtime.join("msedgewebview2.exe").is_file() {
                    std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &runtime);
                }
            }
        }
    }

    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    // The OS share sheet (for the mobile "Share / Save" button) is Android/iOS only.
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_sharesheet::init());

    builder
        .manage(AppState::default())
        .setup(|_app| {
            // On mobile the per-OS data location isn't discoverable via `directories`, so hand the
            // core engine the app's sandboxed data dir for its SQLite store and source cache, and
            // show the window right away (there's no white-flash concern on mobile).
            #[cfg(mobile)]
            {
                use tauri::Manager;
                if let Ok(dir) = _app.path().app_data_dir() {
                    bridgehop_core::paths::set_data_dir(dir);
                }
                if let Some(w) = _app.get_webview_window("main") {
                    let _ = w.show();
                }
            }
            // Desktop keeps the window hidden until the front end has rendered (avoids the blank/
            // white flash) and shows it from onMount. This is a safety net so the window still
            // appears even if that call never fires.
            #[cfg(desktop)]
            {
                use tauri::Manager;
                let handle = _app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(8));
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.show();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::cancel_scan,
            commands::fetch_bridges,
            commands::list_runs,
            commands::reliability,
            commands::clear_history,
            commands::export_bridges,
            commands::save_text_file,
            commands::import_bridges_file,
            commands::qr_svg,
            commands::slipnet_uri,
            commands::deep_status,
            commands::open_external,
            commands::open_pt_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running BridgeHop");
}
