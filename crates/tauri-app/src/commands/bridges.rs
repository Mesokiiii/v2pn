//! Bridges the supervisor's broadcast channels to Tauri events. The
//! webview subscribes to `connection-state` and `log-line`; here we
//! pipe the supervisor's per-state-change and per-log-line broadcasts
//! straight through.

use std::sync::Arc;

use app_core::supervisor::Supervisor;
use tauri::{AppHandle, Emitter};

/// Spawn the two forwarder tasks. Called once during `setup` from
/// `bootstrap::run`. The tasks live for the process lifetime — when
/// Tauri tears down at exit, its async runtime cancels them.
pub fn spawn_event_bridges(app: AppHandle, supervisor: Arc<Supervisor>) {
    {
        let app = app.clone();
        let mut rx = supervisor.subscribe_state();
        tauri::async_runtime::spawn(async move {
            while let Ok(state) = rx.recv().await {
                let _ = app.emit("connection-state", state);
            }
        });
    }
    {
        let app = app.clone();
        let mut rx = supervisor.subscribe_logs();
        tauri::async_runtime::spawn(async move {
            while let Ok(line) = rx.recv().await {
                let _ = app.emit("log-line", &line);
                tokio::task::yield_now().await;
            }
        });
    }
}
