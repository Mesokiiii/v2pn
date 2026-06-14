//! First-boot wiring for the Tauri `setup` callback. Resolves the
//! sing-box binary, creates the runtime directory, runs orphan-state
//! recovery, builds the supervisor, registers `AppState`. Returns the
//! supervisor `Arc` so the daemons module can spawn its monitors with
//! the same handle.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{App, Manager};

use app_core::supervisor::{resolve_singbox_binary, Supervisor};

use crate::commands;

/// Bootstrap result returned to the main `setup` so subsequent steps
/// (daemons, tray, power) can reuse the supervisor handle without
/// re-fetching it through `app.state::<AppState>()`.
pub struct Bootstrapped {
    pub supervisor: Arc<Supervisor>,
}

/// One-stop bootstrap. Order matters:
///  1. Locate the sing-box binary (next to our exe in release, dev
///     fallback otherwise). Hard-fail if missing — there's nothing
///     useful the app can do without it.
///  2. Make sure the runtime directory exists.
///  3. Run orphan-state recovery (kills any stale sing-box left from a
///     previous crashed run; restores the OS proxy if state.json
///     mirrors a half-dead session). Done **before** we let the
///     supervisor try to spawn its own child.
///  4. Build the supervisor + register the AppState into Tauri so
///     command handlers can find it.
///  5. Wire the supervisor's broadcast channels into Tauri events so
///     the frontend gets `connection-state` / `log-line` events.
pub fn run(app: &App) -> anyhow::Result<Bootstrapped> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let binary = resolve_singbox_binary(&exe_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "sing-box binary not found near {}. Run scripts/fetch-singbox.ps1 first.",
            exe_dir.display()
        )
    })?;

    let runtime_dir = app.path().app_data_dir()?.join("runtime");
    std::fs::create_dir_all(&runtime_dir)?;

    // Recovery first — must happen *before* we spawn our own
    // supervisor, because it might have to taskkill an orphan
    // sing-box and free the wintun adapter our new one will claim.
    let _outcome = commands::run_startup_recovery(&runtime_dir);

    let supervisor = Arc::new(Supervisor::new(binary, runtime_dir.clone())?);
    commands::spawn_event_bridges(app.handle().clone(), supervisor.clone());
    app.manage(commands::AppState::new(supervisor.clone(), runtime_dir));

    Ok(Bootstrapped { supervisor })
}
