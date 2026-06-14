//! Long-running background tasks that monitor the engine and self-heal
//! when something goes off the rails. Each daemon is a fire-and-forget
//! `tokio::spawn` — they never need explicit shutdown because the
//! process exit teardown drops their tasks via Tauri's runtime.
//!
//! Public API: [`install_all`] is the only entry-point. Splitting the
//! daemons into named helpers (instead of one mega-spawn) makes it
//! cheap to add / remove / reorder them.

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use app_core::supervisor::{ConnectionState, Supervisor};
use app_core::{state_validator, watchdog};

use crate::commands;

/// Spawn every long-running monitor we care about, in order. Called
/// once during `setup` after `AppState` has been registered.
pub fn install_all(app: &AppHandle, supervisor: Arc<Supervisor>) {
    let opts = tauri::async_runtime::block_on(async {
        app.state::<commands::AppState>().options.lock().await.clone()
    });

    spawn_watchdog(supervisor.clone(), opts.clash_api_port);
    spawn_state_validator(supervisor.clone(), opts.mixed_port, opts.clash_api_port);
    spawn_failed_state_cleanup(app.clone(), supervisor.clone());
    spawn_outbound_health_loop(app.clone(), supervisor);
}

/// Watchdog: pings clash_api every 2 s; on 3 misses asks the supervisor
/// to self-heal (kill + restart from last config). Catches the
/// "sing-box deadlocked but still alive" failure mode.
fn spawn_watchdog(supervisor: Arc<Supervisor>, clash_api_port: u16) {
    let stop = watchdog::new_stop_handle();
    // OnceLock keeps the stop handle alive for the process lifetime so
    // the spawned task is not aborted when this scope exits.
    static HANDLE: OnceLock<watchdog::StopHandle> = OnceLock::new();
    let _ = HANDLE.set(stop.clone());
    tauri::async_runtime::spawn(watchdog::run(supervisor, clash_api_port, stop));
}

/// State validator: every 10 s while Connected, triple-checks
/// (process alive | clash_api responds | mixed-port listening). Two
/// consecutive bad ticks → self-heal. Independent from the watchdog;
/// each catches a different failure class. Both can heal in parallel
/// because the supervisor's auto-restart loop is guarded against
/// duplicate triggers.
fn spawn_state_validator(
    supervisor: Arc<Supervisor>,
    mixed_port: u16,
    clash_api_port: u16,
) {
    let stop = state_validator::new_stop_handle();
    static HANDLE: OnceLock<state_validator::StopHandle> = OnceLock::new();
    let _ = HANDLE.set(stop.clone());
    tauri::async_runtime::spawn(state_validator::run(
        supervisor,
        mixed_port,
        clash_api_port,
        stop,
    ));
}

/// When the supervisor flips to Failed, drop the connection guard so
/// the OS proxy is restored and a future "connect" call doesn't see
/// "already connected". The supervisor's own auto-restart path has
/// already given up at this point.
fn spawn_failed_state_cleanup(app: AppHandle, supervisor: Arc<Supervisor>) {
    let mut state_rx = supervisor.subscribe_state();
    tauri::async_runtime::spawn(async move {
        while let Ok(s) = state_rx.recv().await {
            if matches!(s, ConnectionState::Failed { .. }) {
                tracing::warn!(target: "auto-cleanup",
                    "engine reported Failed — releasing proxy guard");
                let state = app.state::<commands::AppState>();
                commands::release_guard_after_failure(&state).await;
            }
        }
    });
}

/// Outbound health probe loop. While Connected, every
/// `HEALTH_INTERVAL`, asks the clash API to dial the currently
/// selected outbound through the public probe URL. Result is broadcast
/// via the `outbound-health` Tauri event so the UI lights up the 🟢/
/// 🟡/🔴 badge. Distinct from the watchdog: that one polls the clash
/// API itself (sing-box liveness); this one polls the upstream tunnel
/// (server liveness).
fn spawn_outbound_health_loop(app: AppHandle, supervisor: Arc<Supervisor>) {
    /// Cadence after the warmup tick.
    const HEALTH_INTERVAL: Duration = Duration::from_secs(20);
    /// Grace period after Connected → first probe. Lets the freshly-
    /// spawned proxy settle (TLS handshake, REALITY exchange, …).
    const FIRST_PROBE_DELAY: Duration = Duration::from_secs(3);

    tauri::async_runtime::spawn(async move {
        let mut warmed_up = false;
        loop {
            let interval = if warmed_up { HEALTH_INTERVAL } else { FIRST_PROBE_DELAY };
            tokio::time::sleep(interval).await;

            if !matches!(supervisor.state(), ConnectionState::Connected) {
                warmed_up = false;
                continue;
            }
            warmed_up = true;

            let port = {
                let state = app.state::<commands::AppState>();
                let p = state.options.lock().await.clash_api_port;
                p
            };
            let secret = supervisor.clash_secret();

            let Some(tag) = app_core::outbound_health::current_active_tag(
                port,
                secret.as_deref(),
            )
            .await
            else {
                tracing::debug!(target: "v2pn::health",
                    "no active outbound tag yet, skipping probe");
                continue;
            };

            let h = app_core::outbound_health::probe(port, &tag, secret.as_deref()).await;
            if h.latency_ms.is_none() {
                tracing::warn!(
                    target: "v2pn::health",
                    tag = %h.tag,
                    error = ?h.error,
                    "outbound probe failed"
                );
            } else {
                tracing::debug!(
                    target: "v2pn::health",
                    tag = %h.tag,
                    latency_ms = h.latency_ms,
                    "outbound probe ok"
                );
            }
            let _ = app.emit("outbound-health", &h);
        }
    });
}
