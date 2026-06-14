//! Tauri IPC command surface, decomposed by domain.
//!
//! Each submodule owns one cohesive set of related commands; this
//! `mod.rs` is the facade that re-exports everything `main.rs` knew
//! about under the old single-file layout — adding a new command
//! means dropping a `pub fn` into the right submodule plus one
//! `pub use` line here, no churn at the call sites.
//!
//! Submodules:
//!  - [`subscription`] — fetch + parse user subscription URLs.
//!  - [`connection`]   — connect / disconnect / switch_server +
//!                        `shutdown_session` and `release_guard_after_failure`.
//!  - [`routing`]      — set_routing / get_options / set_mode +
//!                        latency probe batch.
//!  - [`diagnostics`]  — ping / repair_network / open_logs_folder /
//!                        diagnostics dump.
//!  - [`elevation`]    — UAC status + restart-as-admin.
//!  - [`bridges`]      — supervisor → Tauri event bridges
//!                        (`spawn_event_bridges`).
//!  - [`recovery`]     — startup orphan-state cleanup
//!                        (`run_startup_recovery`).
//!
//! Cross-cutting types (`AppState`, `CommandError`, `LastConnectIntent`,
//! `ShutdownOpts`) live here at the top level so submodules don't have
//! to chase imports across each other.

use std::path::PathBuf;
use std::sync::Arc;

use app_core::profile::ProxyProfile;
use app_core::singbox::config::{ConnectionMode, ConnectionOptions};
use app_core::state_guard::ConnectionGuard;
use app_core::supervisor::Supervisor;
use serde::Serialize;
use tokio::sync::Mutex;

// Submodules are `pub` so the `tauri::generate_handler!` macro in
// main.rs can reach the per-command `__cmd__<name>` symbols that
// `#[tauri::command]` generates next to each function (those symbols
// are not visible through a plain `pub use` re-export).
pub mod bridges;
pub mod connection;
pub mod diagnostics;
pub mod elevation;
pub mod recovery;
pub mod routing;
pub mod subscription;

// Re-exports for non-command helpers / shared types so existing call
// sites (`commands::shutdown_session`, `commands::spawn_event_bridges`,
// `commands::run_startup_recovery`, `commands::ShutdownOpts`,
// `commands::connect_subscription_internal`) keep working without
// touching the rest of the codebase.
pub use bridges::spawn_event_bridges;
pub use connection::{
    connect_subscription_internal, release_guard_after_failure, shutdown_session,
    ShutdownOpts,
};
pub use recovery::run_startup_recovery;

/// Common error type for every Tauri command. Rendered on the frontend
/// as `CommandError { message: string }`. Implements `From<E: Display>`
/// so `?` works for any anyhow / std error.
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        CommandError {
            message: e.to_string(),
        }
    }
}

/// Snapshot of "what the user last asked us to connect to". Captured at
/// the end of every successful connect, used by the suspend/resume
/// handler to bring the same connection back automatically after the
/// laptop wakes up.
#[derive(Clone)]
pub struct LastConnectIntent {
    pub profiles: Vec<ProxyProfile>,
    pub selected_id: String,
    pub mode: ConnectionMode,
}

/// Shared application state held by Tauri. Cloned trivially via inner
/// `Arc`s; safe to grab from any command handler via
/// `state: State<'_, AppState>`.
pub struct AppState {
    pub supervisor: Arc<Supervisor>,
    pub options: Arc<Mutex<ConnectionOptions>>,
    /// Active proxy guard. `None` while disconnected.
    pub guard: Arc<Mutex<Option<ConnectionGuard>>>,
    /// Profile id currently routed by the running sing-box (`None`
    /// while idle). Used by the frontend to know whether
    /// `switch_server` is applicable.
    pub active_selected: Arc<Mutex<Option<String>>>,
    /// Snapshot of what the user last connected to. Used by the
    /// suspend/resume handler. Cleared by an explicit `disconnect`.
    pub last_intent: Arc<Mutex<Option<LastConnectIntent>>>,
    /// Was the user connected when the system started suspending? Set
    /// in the `Suspend` branch of the power handler, consumed in
    /// `Resume` to decide whether to auto-reconnect or stay idle.
    pub suspend_was_connected: Arc<Mutex<bool>>,
    pub state_dir: PathBuf,
}

impl AppState {
    pub fn new(supervisor: Arc<Supervisor>, state_dir: PathBuf) -> Self {
        Self {
            supervisor,
            options: Arc::new(Mutex::new(ConnectionOptions::default())),
            guard: Arc::new(Mutex::new(None)),
            active_selected: Arc::new(Mutex::new(None)),
            last_intent: Arc::new(Mutex::new(None)),
            suspend_was_connected: Arc::new(Mutex::new(false)),
            state_dir,
        }
    }
}
