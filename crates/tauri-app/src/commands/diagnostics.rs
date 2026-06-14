//! Diagnostics & misc utility commands: ping, repair_network,
//! open_logs_folder, diagnostics dump.

use tauri::{AppHandle, Manager, State};

use super::{AppState, CommandError};

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

/// "Fix my network" — emergency cleanup the user can run if some other
/// VPN tool (or our own crash) left the OS networking stack in a
/// half-broken state. Stops sing-box, force-clears the system proxy,
/// removes stale Wintun adapters, flushes DNS / ARP caches, notifies
/// Wininet so browsers re-read the registry. Returns a per-step report
/// the UI renders as a timeline.
#[tauri::command]
pub async fn repair_network(
    state: State<'_, AppState>,
) -> Result<app_core::network_repair::RepairReport, CommandError> {
    // Wipe any in-flight intent so post-repair the user has to opt
    // back in instead of the auto-resume handler firing.
    *state.last_intent.lock().await = None;
    *state.suspend_was_connected.lock().await = false;
    *state.active_selected.lock().await = None;
    if let Some(g) = state.guard.lock().await.take() {
        let _ = g.release();
    }

    let opts = state.options.lock().await.clone();
    let report = app_core::network_repair::run_full_repair(
        state.supervisor.clone(),
        &opts.tun_interface_name,
    )
    .await;
    Ok(report)
}

#[tauri::command]
pub async fn open_logs_folder(app: AppHandle) -> Result<String, CommandError> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError { message: e.to_string() })?
        .join("logs");
    let _ = std::fs::create_dir_all(&path);
    let path_str = path.to_string_lossy().to_string();
    tracing::info!(target: "v2pn::cmd", path = %path_str, "open_logs_folder");
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(&path_str).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&path_str).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&path_str).spawn();
    }
    Ok(path_str)
}

/// Self-diagnostic snapshot — useful for bug reports. Includes
/// versions, elevation, current connection state, port usage, hwid
/// prefix.
#[tauri::command]
pub async fn diagnostics(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let opts = state.options.lock().await.clone();
    let active = state.active_selected.lock().await.clone();
    let guard_present = state.guard.lock().await.is_some();
    let elev = app_core::elevation::is_elevated();

    Ok(serde_json::json!({
        "v2pn_version": env!("CARGO_PKG_VERSION"),
        "rust_target": cfg_target(),
        "elevated": elev.elevated,
        "elevation_supported": elev.supported,
        "options": opts,
        "active_server_id": active,
        "guard_present": guard_present,
        "supervisor_state": state.supervisor.state(),
        "hwid_prefix": &app_core::hwid::hwid()[..8],
    }))
}

#[inline]
fn cfg_target() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}
