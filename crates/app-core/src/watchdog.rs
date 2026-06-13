//! Liveness watchdog for the sing-box sidecar.
//!
//! Polls the clash API every 2 seconds. Three consecutive failures while the
//! supervisor reports `Connected` triggers an emergency stop, which cascades
//! through the Drop chain and restores the system proxy.
//!
//! The function is async and runtime-agnostic: callers spawn it with their
//! preferred executor (Tauri uses `tauri::async_runtime::spawn`).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use crate::supervisor::{ConnectionState, Supervisor};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const FAILURE_BUDGET: u32 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);

/// Stop handle. Drop or `notify_one()` to terminate the loop.
pub type StopHandle = Arc<Notify>;

pub fn new_stop_handle() -> StopHandle {
    Arc::new(Notify::new())
}

/// Returns the watchdog `Future`. Spawn it with your runtime of choice.
pub async fn run(supervisor: Arc<Supervisor>, clash_api_port: u16, stop: StopHandle) {
    let url = format!("http://127.0.0.1:{}/version", clash_api_port);
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(target: "watchdog", "client build failed: {e}");
            return;
        }
    };

    let mut consecutive_failures: u32 = 0;
    loop {
        tokio::select! {
            _ = stop.notified() => {
                tracing::debug!(target: "watchdog", "stopped");
                return;
            }
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }

        if !matches!(supervisor.state(), ConnectionState::Connected) {
            consecutive_failures = 0;
            continue;
        }

        match {
            let mut req = client.get(&url);
            if let Some(s) = supervisor.clash_secret() {
                req = req.bearer_auth(s);
            }
            req.send().await
        } {
            Ok(resp) if resp.status().is_success() => {
                if consecutive_failures > 0 {
                    tracing::info!(target: "watchdog",
                        "clash_api recovered after {consecutive_failures} miss(es)");
                }
                consecutive_failures = 0;
            }
            outcome => {
                consecutive_failures += 1;
                tracing::warn!(target: "watchdog",
                    "clash_api miss {}/{} ({:?})", consecutive_failures, FAILURE_BUDGET, outcome.err());
                if consecutive_failures >= FAILURE_BUDGET {
                    tracing::error!(target: "watchdog",
                        "engine unresponsive — requesting self-heal");
                    // Self-heal instead of bailing out. The supervisor
                    // brings sing-box back up automatically using the
                    // last good config; users keep their connection
                    // across hiccups instead of being kicked back to a
                    // disconnected state. If self-heal itself fails,
                    // the auto-restart loop's backoff schedule decides
                    // when to give up.
                    supervisor.request_self_heal("watchdog: clash_api unresponsive").await;
                    consecutive_failures = 0;
                }
            }
        }
    }
}
