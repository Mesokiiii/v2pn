//! OS power-state handlers. The OS yells at us when it's about to
//! suspend (laptop closing, sleep timer, hibernate) and again on
//! resume; we use those edges to do graceful disconnect / auto-replay.
//!
//! Public API: [`install`].

use tauri::{AppHandle, Manager};

use app_core::power::{register as register_power, PowerEvent};

use crate::commands;

/// Register the OS-level Suspend/Resume callback. Must be called exactly
/// once during `setup` — the underlying Win32 hook is process-global and
/// we never unregister it (the process exits with us).
///
/// Suspend sequence:
///   1. Snapshot whether we were Connected → `suspend_was_connected`.
///   2. Run [`commands::shutdown_session`] with `ShutdownOpts::SUSPEND`,
///      which kills sing-box but **keeps** `last_intent` so Resume can
///      replay it.
///
/// Resume sequence:
///   1. Read+clear `suspend_was_connected` (defends against double-fire
///      that some Windows configurations produce across the
///      S3↔Modern-Standby boundary).
///   2. If we were connected, sleep 3 s (lets Wi-Fi / Ethernet drivers
///      come back), then replay the saved [`commands::LastConnectIntent`]
///      through the regular connect command.
pub fn install(app: &AppHandle) {
    let suspend_handle = app.clone();
    let resume_handle = app.clone();

    register_power(move |event| match event {
        PowerEvent::Suspend => on_suspend(suspend_handle.clone()),
        PowerEvent::Resume => on_resume(resume_handle.clone()),
    });
}

fn on_suspend(app: AppHandle) {
    tracing::warn!(target: "power", "system suspending — emergency disconnect");
    tauri::async_runtime::spawn(async move {
        // Capture the connection state *before* tear-down — the
        // supervisor flips to Stopping/Idle the moment the shutdown
        // helper runs, so reading after is too late.
        let was_connected = {
            let state = app.state::<commands::AppState>();
            let was = matches!(
                state.supervisor.state(),
                app_core::supervisor::ConnectionState::Connected
                    | app_core::supervisor::ConnectionState::Starting
            );
            *state.suspend_was_connected.lock().await = was;
            was
        };
        let _ = was_connected; // recorded for the resume side
        let state = app.state::<commands::AppState>();
        commands::shutdown_session(&state, &app, commands::ShutdownOpts::SUSPEND).await;
    });
}

fn on_resume(app: AppHandle) {
    tracing::info!(target: "power", "system resumed");
    tauri::async_runtime::spawn(async move {
        // Read+clear the suspend flag and snapshot the saved intent in
        // a single critical section so a duplicate Resume notification
        // can't trigger two reconnects.
        let (was_connected, last_intent) = {
            let state = app.state::<commands::AppState>();
            let was = *state.suspend_was_connected.lock().await;
            *state.suspend_was_connected.lock().await = false;
            let intent = state.last_intent.lock().await.clone();
            (was, intent)
        };

        if !was_connected {
            tracing::debug!(target: "power",
                "resume: not previously connected — staying idle");
            return;
        }
        let Some(intent) = last_intent else {
            tracing::warn!(target: "power",
                "resume: was connected but no last_intent recorded; can't auto-reconnect");
            return;
        };

        // Hold for the network stack to settle. 3 s is the sweet spot
        // we measured on the test laptop: less than 1 s and reqwest
        // tends to grab a stale route, more than 5 s and the user
        // notices the VPN-down window.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        tracing::info!(target: "power",
            profiles = intent.profiles.len(),
            selected = %intent.selected_id,
            mode = ?intent.mode,
            "auto-reconnecting after resume");

        let app_state = app.state::<commands::AppState>();
        let res = commands::connect_subscription_internal(
            intent.profiles,
            intent.selected_id,
            Some(intent.mode),
            app_state,
            app.clone(),
        )
        .await;
        if let Err(e) = res {
            tracing::error!(target: "power",
                "auto-reconnect failed: {}", e.message);
        }
    });
}
