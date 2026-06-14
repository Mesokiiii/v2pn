//! UAC elevation status query + relaunch-as-admin command.

use tauri::{AppHandle, Manager};

use super::{AppState, CommandError};

#[tauri::command]
pub fn elevation_status() -> app_core::elevation::ElevationStatus {
    app_core::elevation::is_elevated()
}

/// Re-launch the current process via Windows UAC. On success, this
/// terminates the caller — the elevated copy takes over.
#[tauri::command]
pub async fn restart_as_admin(app: AppHandle) -> Result<(), CommandError> {
    // Tear down state in the right order so the elevated copy starts
    // on a clean canvas: stop sing-box (releases the kill-on-close Job
    // Object handle), drop the guard (restores OS proxy snapshot),
    // close every window so the user doesn't see two v2pn icons in
    // the taskbar during the swap.
    if let Some(state) = app.try_state::<AppState>() {
        let _ = state.supervisor.stop().await;
        let taken = { state.guard.lock().await.take() };
        if let Some(g) = taken {
            let _ = g.release();
        }
    }
    for (_, w) in app.webview_windows() {
        let _ = w.close();
    }

    // Spawn the elevated copy. ShellExecuteW returns the moment
    // Windows approves the UAC prompt; the elevated child is queued
    // but hasn't started yet.
    app_core::elevation::restart_as_admin()
        .map_err(|e| CommandError { message: format!("UAC: {e}") })?;

    // CRITICAL: terminate ourselves *synchronously* before the
    // elevated child enters its `tauri-plugin-single-instance` mutex
    // check.
    //
    // Bug history: we used to call `app.exit(0)` here, which is
    // async — it merely *requests* the Tauri event loop to wind
    // down. Meanwhile the elevated child started ~50 ms later,
    // observed our mutex still alive, and the single-instance plugin
    // sent the focus event to us and silently shut itself down. End
    // result: the user clicked "Restart as admin", saw a UAC prompt,
    // and ended up with the same un-elevated v2pn they started with.
    //
    // `std::process::exit` skips Tauri's drop chain entirely, but
    // (a) we already manually released supervisor / guard above, so
    // the important cleanup ran, and (b) Windows reclaims all kernel
    // handles — including the single-instance mutex — at process
    // termination, which is exactly the race we needed to win.
    std::process::exit(0);
}
