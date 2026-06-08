//! Shared application state: the cancellation handle for the in-flight scan.

use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

/// Tracks the currently running scan so it can be cancelled from the UI.
#[derive(Default)]
pub struct AppState {
    current: Mutex<Option<CancellationToken>>,
}

impl AppState {
    /// Register a new scan, cancelling any previous one still running.
    pub fn begin(&self, token: CancellationToken) {
        if let Ok(mut guard) = self.current.lock() {
            if let Some(previous) = guard.replace(token) {
                previous.cancel();
            }
        }
    }

    /// Clear the handle once a scan completes.
    pub fn finish(&self) {
        if let Ok(mut guard) = self.current.lock() {
            *guard = None;
        }
    }

    /// Cancel the in-flight scan, if any.
    pub fn cancel(&self) {
        if let Ok(guard) = self.current.lock() {
            if let Some(token) = guard.as_ref() {
                token.cancel();
            }
        }
    }
}
