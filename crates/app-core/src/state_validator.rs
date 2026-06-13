//! Periodic three-way consistency check between the supervisor's view of
//! reality and what actually exists on the box.
//!
//! Every `INTERVAL` while the supervisor reports `Connected` we cross-
//! validate three independent signals:
//!
//!   1. **Process alive** — sing-box's PID is still in the process table
//!      (use `process_guard::list_singbox_pids` rather than holding the
//!      Tokio child handle to avoid taking the supervisor mutex).
//!   2. **Clash API responsive** — `GET /version` returns 200 within
//!      `HTTP_TIMEOUT`. Catches sing-box that's alive but deadlocked.
//!   3. **Mixed-port listening** — `TcpStream::connect("127.0.0.1:<port>")`
//!      succeeds. Catches the case where sing-box is alive AND clash_api
//!      responds but the proxy listener got dropped (very rare but cheap
//!      to verify).
//!
//! Any one of these failing for two consecutive ticks is treated as a
//! consistency violation; we ask the supervisor to self-heal (kill +
//! restart from the last known config). One-off failures are tolerated to
//! ride out transient hiccups (process scan races, network stack pauses).
//!
//! Distinct from `watchdog`:
//!   * watchdog only pings clash_api; if it misses thrice it `stop()`s.
//!   * state_validator triple-checks invariants and *self-heals* instead
//!     of stopping — the user keeps their connection across hiccups.
//!
//! Both can run together; the auto-restart loop is idempotent (guarded
//! by an AtomicBool inside the supervisor) so duplicate triggers fold.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use crate::supervisor::{ConnectionState, Supervisor};

/// How often the validator wakes up. 10s is far enough apart that the
/// per-tick cost is negligible (one local TCP, one HTTP request, one
/// process snapshot ~ a few ms total) and short enough that any real
/// outage is acted on before users notice.
const INTERVAL: Duration = Duration::from_secs(10);

/// HTTP timeout for the clash API probe. Matched to `watchdog` so the two
/// tasks see the same world-view.
const HTTP_TIMEOUT: Duration = Duration::from_millis(1500);

/// Number of consecutive ticks with at least one failing signal before we
/// trip a self-heal. `2` gives roughly 20 s of grace, which covers a
/// power-resume race or a brief NIC reset without flapping.
const FAIL_BUDGET: u32 = 2;

/// Stop-handle alias — caller drops or notifies to terminate the loop.
pub type StopHandle = Arc<Notify>;

pub fn new_stop_handle() -> StopHandle {
    Arc::new(Notify::new())
}

/// Async entry point. Spawn with the runtime of your choice; in the
/// Tauri host we use `tauri::async_runtime::spawn`. The function only
/// returns when `stop` is notified or dropped.
pub async fn run(supervisor: Arc<Supervisor>, mixed_port: u16, clash_api_port: u16, stop: StopHandle) {
    let clash_url = format!("http://127.0.0.1:{}/version", clash_api_port);
    let client = match reqwest::Client::builder()
        .no_proxy()
        .timeout(HTTP_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(target: "validator", "client build failed: {e}");
            return;
        }
    };

    let mut consecutive_fail: u32 = 0;

    loop {
        tokio::select! {
            _ = stop.notified() => {
                tracing::debug!(target: "validator", "stopped");
                return;
            }
            _ = tokio::time::sleep(INTERVAL) => {}
        }

        // Only validate while we *should* have a healthy connection.
        // `Starting` is a transitional grace period (sing-box is still
        // doing its startup work); `Idle`/`Stopping`/`Failed` mean the
        // supervisor has nothing for us to verify.
        if !matches!(supervisor.state(), ConnectionState::Connected) {
            consecutive_fail = 0;
            continue;
        }

        let mut failed_signals: Vec<&'static str> = Vec::new();

        // Signal 1: process alive. We compare the supervisor's PID to the
        // live process table — if its PID isn't in there, the child
        // disappeared without the death-watcher having caught up yet.
        let expected_pid = supervisor.child_pid().await;
        if let Some(pid) = expected_pid {
            let alive_pids = crate::process_guard::list_singbox_pids();
            if !alive_pids.contains(&pid) {
                failed_signals.push("process gone");
            }
        } else {
            // The supervisor reports Connected without a PID — definitely
            // inconsistent. Probably a race where the child slot was just
            // taken; we'll catch it on the next tick if it doesn't clear.
            failed_signals.push("supervisor.child_pid is None despite Connected");
        }

        // Signal 2: clash_api responsive. A 200 from /version means the
        // entire HTTP stack is alive inside sing-box, which transitively
        // implies the goroutine scheduler is fine.
        let resp_result = {
            let mut req = client.get(&clash_url);
            if let Some(s) = supervisor.clash_secret() {
                req = req.bearer_auth(s);
            }
            req.send().await
        };
        match resp_result {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => failed_signals.push(match r.status().as_u16() {
                404 => "clash_api 404 (server up but unexpected version handler)",
                401 | 403 => "clash_api 401/403 (secret out of sync)",
                _ => "clash_api non-2xx",
            }),
            Err(_) => failed_signals.push("clash_api unreachable"),
        }

        // Signal 3: mixed-port listening. A successful TCP connect to the
        // local proxy port is the cheapest end-to-end check. We use
        // `tokio::net` so we honour the executor's reactor.
        let mixed_addr = format!("127.0.0.1:{mixed_port}");
        match tokio::time::timeout(
            Duration::from_millis(500),
            tokio::net::TcpStream::connect(&mixed_addr),
        )
        .await
        {
            Ok(Ok(_stream)) => { /* the connect itself is the assertion */ }
            Ok(Err(e)) => failed_signals.push(match e.kind() {
                std::io::ErrorKind::ConnectionRefused => "mixed-port not listening",
                _ => "mixed-port connect failed",
            }),
            Err(_) => failed_signals.push("mixed-port connect timed out"),
        }

        if failed_signals.is_empty() {
            if consecutive_fail > 0 {
                tracing::info!(
                    target: "validator",
                    "all signals green again after {consecutive_fail} bad tick(s)"
                );
            }
            consecutive_fail = 0;
            continue;
        }

        consecutive_fail += 1;
        tracing::warn!(
            target: "validator",
            "tick {}/{}: failed signals: {}",
            consecutive_fail,
            FAIL_BUDGET,
            failed_signals.join(", ")
        );

        if consecutive_fail >= FAIL_BUDGET {
            tracing::error!(
                target: "validator",
                "consistency budget exhausted ({} consecutive bad ticks); requesting self-heal",
                consecutive_fail
            );
            // Don't stop — *heal*. The supervisor restarts sing-box from
            // its last good config, the watchdog gets a fresh PID via
            // child_pid(), and we keep going.
            supervisor.request_self_heal("state validator: consistency violation").await;
            consecutive_fail = 0;
        }
    }
}
