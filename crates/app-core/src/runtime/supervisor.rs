//! sing-box sidecar supervisor.
//!
//! Responsibilities
//! ================
//! - resolve the bundled `sing-box` binary path
//! - write the generated config to a temp file (mode 0600 on Unix)
//! - spawn `sing-box run -c <file>`, capture stdout/stderr line-by-line
//! - signal connection state (`Idle` / `Starting` / `Connected` / `Failed{reason}` / `Stopping`)
//! - graceful shutdown with timeout, force kill if needed
//!
//! The supervisor is *not* aware of Tauri. The Tauri layer wires the
//! emitted events into webview events.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::error::{CoreError, CoreResult};
use crate::singbox::config::ConnectionMode;

/// Snapshot of the supervisor lifecycle as visible to the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ConnectionState {
    Idle,
    Starting,
    Connected,
    Failed { reason: String },
    Stopping,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Idle
    }
}

/// One log line from sing-box, broadcast to subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    /// Path to the sing-box binary. Resolved once at construction time.
    binary: PathBuf,
    /// Working directory where the active config file lives.
    runtime_dir: PathBuf,
    /// Currently-running child + the cleanup join handle.
    child: Mutex<Option<RunningChild>>,
    state: RwLock<ConnectionState>,
    state_tx: broadcast::Sender<ConnectionState>,
    log_tx: broadcast::Sender<LogLine>,
    /// Windows Job Object that auto-kills any assigned child if our own
    /// process dies, no matter how (graceful exit, panic, kill -9, BSOD,
    /// power loss). The OS enforces this — it's the strongest guarantee
    /// we can make against orphaned sidecars. `None` only if Job Object
    /// creation failed at startup, in which case we degrade to the older
    /// `kill_on_drop` + state-file recovery path.
    job_guard: Option<crate::process_guard::ProcessJobGuard>,
    /// The last config + mode we successfully started with. The
    /// auto-restart loop reads this when sing-box dies unexpectedly so we
    /// can bring it back without waiting for the user to click reconnect.
    /// Cleared by an explicit `stop()`.
    last_config: Mutex<Option<RestartContext>>,
    /// Set to `true` when the user explicitly disconnects. While `true`,
    /// the death watcher does NOT spawn the auto-restart loop, even if
    /// sing-box dies during the shutdown sequence. Reset to `false` at
    /// the top of `start()`.
    user_initiated_stop: AtomicBool,
    /// Set to `true` while an auto-restart loop is in flight, so we don't
    /// spawn two of them in parallel (e.g. death-watcher fires AND the
    /// state validator fires within the same tick).
    auto_restart_in_flight: AtomicBool,
    /// Random per-connection secret that authenticates our HTTP calls to
    /// the sing-box clash API. Without this, any local user / process /
    /// browser tab on the box could hijack the proxy by PUT-ing to
    /// `/proxies/proxy`. Generated fresh in `start()`, propagated into
    /// the config via `experimental.clash_api.secret`, and read by every
    /// clash_api caller (switch_server, outbound_health, watchdog,
    /// state_validator).
    clash_secret: RwLock<Option<String>>,
}

/// Snapshot of the inputs we need to bring sing-box back up. Stored in
/// `SupervisorInner.last_config` after every successful `start()` and read
/// by the auto-restart loop. Both fields are owned values (no borrows)
/// so the loop can keep them across awaits without holding the mutex.
#[derive(Clone)]
struct RestartContext {
    config: serde_json::Value,
    mode: ConnectionMode,
}

struct RunningChild {
    child: Child,
    config_path: PathBuf,
    mode: ConnectionMode,
    /// PID of the spawned sing-box (0 if Tokio's id() returned None,
    /// which only happens on already-exited handles). Captured at spawn
    /// time so recovery on the next launch can target this exact PID
    /// even if the OS reused it for someone else — we cross-reference
    /// against the executable name in `process_guard::list_singbox_pids`.
    pid: u32,
    pumps: Vec<JoinHandle<()>>,
}

impl Supervisor {
    pub fn new(binary: PathBuf, runtime_dir: PathBuf) -> CoreResult<Self> {
        if !binary.exists() {
            return Err(CoreError::Other(format!(
                "sing-box binary not found: {}",
                binary.display()
            )));
        }
        std::fs::create_dir_all(&runtime_dir)?;
        let (state_tx, _) = broadcast::channel(8);
        let (log_tx, _) = broadcast::channel(2048);

        // Job Object creation is best-effort: if it fails (very rare —
        // would mean the kernel is out of handles), we still run, just
        // without the OS-enforced kill-on-close guarantee. The recovery
        // path on the next start handles that fallback by killing any
        // orphan sing-box processes left behind.
        let job_guard = match crate::process_guard::ProcessJobGuard::create_kill_on_close() {
            Ok(g) => {
                info!(target: "supervisor", "kill-on-close Job Object armed");
                Some(g)
            }
            Err(e) => {
                warn!(target: "supervisor",
                    "Job Object unavailable, falling back to kill_on_drop only: {e}");
                None
            }
        };

        Ok(Self {
            inner: Arc::new(SupervisorInner {
                binary,
                runtime_dir,
                child: Mutex::new(None),
                state: RwLock::new(ConnectionState::Idle),
                state_tx,
                log_tx,
                job_guard,
                last_config: Mutex::new(None),
                user_initiated_stop: AtomicBool::new(false),
                auto_restart_in_flight: AtomicBool::new(false),
                clash_secret: RwLock::new(None),
            }),
        })
    }

    pub fn state(&self) -> ConnectionState {
        self.inner.state.read().clone()
    }

    /// PID of the currently-running sing-box, or `None` if not running.
    /// Surfaced for diagnostics and for the state validator that
    /// cross-checks `Connected` ↔ live process every few seconds.
    pub async fn child_pid(&self) -> Option<u32> {
        self.inner
            .child
            .lock()
            .await
            .as_ref()
            .map(|rc| rc.pid)
            .filter(|p| *p != 0)
    }

    /// The clash_api secret currently in effect. Returned by `start()`
    /// after it generates a fresh random token and injects it into the
    /// config. Every clash_api caller (switch, outbound_health, validator,
    /// watchdog) attaches it as `Authorization: Bearer <secret>`. The
    /// returned `Option` is `None` only between processes — i.e. before
    /// the first `start()` and after `stop()`.
    pub fn clash_secret(&self) -> Option<String> {
        self.inner.clash_secret.read().clone()
    }

    /// Generate a fresh 256-bit random secret, JSON-inject it into
    /// `experimental.clash_api.secret`, and stash it in the supervisor.
    /// Called by `connect_inner` *after* `sanitize_strict` has scrubbed
    /// any attacker-supplied secret from the untrusted subscription
    /// config. Anything subsequently dialing the clash API has to attach
    /// `Authorization: Bearer <secret>` or sing-box will respond 401.
    ///
    /// We use the OS CSPRNG via the `getrandom` shim shipped by `uuid`'s
    /// `v4` impl — same source the rest of the project uses for IDs, so
    /// no new transitive dependency.
    pub fn rotate_clash_secret(&self, cfg: &mut serde_json::Value) -> String {
        // 32 hex chars (= 128 bits of entropy) are plenty against an
        // online attacker pinned to localhost — clash_api also accepts
        // longer values without issue. We use a fresh UUIDv4 squashed to
        // its hex form so we don't pull in another RNG crate.
        let secret = uuid::Uuid::new_v4().simple().to_string();
        // Make sure `experimental.clash_api` exists before writing into it.
        // Most code paths hand us a config that already has it (config.rs
        // builds it for both proxy and TUN modes), but be defensive — a
        // future refactor that omits clash_api shouldn't silently produce
        // an unauthenticated daemon.
        let exp = cfg
            .as_object_mut()
            .expect("config root is object")
            .entry("experimental")
            .or_insert_with(|| serde_json::json!({}));
        let api = exp
            .as_object_mut()
            .expect("experimental is object")
            .entry("clash_api")
            .or_insert_with(|| serde_json::json!({}));
        api.as_object_mut()
            .expect("clash_api is object")
            .insert("secret".into(), serde_json::Value::String(secret.clone()));
        *self.inner.clash_secret.write() = Some(secret.clone());
        secret
    }

    pub fn subscribe_state(&self) -> broadcast::Receiver<ConnectionState> {
        self.inner.state_tx.subscribe()
    }

    pub fn subscribe_logs(&self) -> broadcast::Receiver<LogLine> {
        self.inner.log_tx.subscribe()
    }

    /// Start sing-box with the given (already-sanitised) JSON config.
    pub async fn start(&self, config: &serde_json::Value, mode: ConnectionMode) -> CoreResult<()> {
        // Reject if a *live* child is already running. If the slot holds a
        // corpse left over from a crashed previous run (death_watcher races,
        // panicked drop, etc.) — reap it and continue. Without this branch
        // any unclean exit permanently wedges the supervisor with
        // "sing-box already running; stop it first".
        {
            let mut guard = self.inner.child.lock().await;
            if let Some(rc) = guard.as_mut() {
                match rc.child.try_wait() {
                    Ok(Some(status)) => {
                        warn!(?status, "reaping stale dead child before restart");
                        let mut stale = guard.take().unwrap();
                        for h in stale.pumps.drain(..) { h.abort(); }
                        if stale.config_path.exists() {
                            let _ = tokio::fs::remove_file(&stale.config_path).await;
                        }
                        // TUN drivers (wintun in particular) need a moment
                        // to release the virtual adapter after the owning
                        // process dies. Same rationale as the grace sleep
                        // in stop().
                        if matches!(stale.mode, ConnectionMode::Tun) {
                            tokio::time::sleep(Duration::from_millis(800)).await;
                        }
                    }
                    Ok(None) => {
                        return Err(CoreError::Other(
                            "sing-box already running; stop it first".into(),
                        ));
                    }
                    Err(e) => {
                        warn!(?e, "try_wait failed during start; assuming alive");
                        return Err(CoreError::Other(
                            "sing-box already running; stop it first".into(),
                        ));
                    }
                }
            }
        }

        self.set_state(ConnectionState::Starting);

        // Reset the user-stop flag at the very top of `start()`. After this
        // point, any unexpected child death triggers auto-restart. The
        // flag stays `false` for the entire connected lifetime; only an
        // explicit `stop()` (i.e. user pressed Disconnect) flips it back.
        self.inner.user_initiated_stop.store(false, Ordering::Release);

        let config_path = self.inner.runtime_dir.join("active.json");
        let pretty = serde_json::to_vec_pretty(config)?;
        tokio::fs::write(&config_path, &pretty).await?;

        // Open a fresh sing-box.log (truncate previous run). Used by both
        // pump_lines and humans tail-ing the file directly.
        let log_path = self.inner.runtime_dir.join("sing-box.log");
        let log_file = std::sync::Arc::new(std::sync::Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_path)
                .ok(),
        ));

        let mut cmd = Command::new(&self.inner.binary);
        cmd.arg("run")
            .arg("-c").arg(&config_path)
            .arg("-D").arg(&self.inner.runtime_dir)
            .env("NO_COLOR", "1") // suppress ANSI colour codes on stderr
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Hide the console window on Windows. Without this the sidecar pops
        // a black cmd.exe window every time the supervisor restarts.
        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let reason = format!("spawn failed: {e}");
                self.set_state(ConnectionState::Failed { reason: reason.clone() });
                return Err(CoreError::Other(reason));
            }
        };

        // Place the child into our kill-on-close Job Object. From this
        // point on, the OS itself guarantees the child dies if v2pn ever
        // does — kill -9, BSOD, panic skipping destructors, anything.
        // We do this *immediately* after spawn, before any logic that
        // could fail, so even an early return below leaves a child the
        // kernel will reap.
        let child_pid = child.id().unwrap_or(0);
        if let Some(job) = &self.inner.job_guard {
            if child_pid == 0 {
                warn!(target: "supervisor", "spawned child has no PID; cannot assign to job");
            } else if let Err(e) = job.assign(child_pid) {
                // Non-fatal: kill_on_drop still applies, recovery still runs
                // on the next launch. We just lose the BSOD-resistance.
                warn!(target: "supervisor",
                    "AssignProcessToJobObject({child_pid}) failed: {e}; falling back to kill_on_drop");
            }
        }
        info!(target: "supervisor", pid = child_pid, "sing-box spawned");

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let log_tx = self.inner.log_tx.clone();
        let pump_out = tokio::spawn(pump_lines(
            stdout, LogStream::Stdout, log_tx.clone(), log_file.clone(),
        ));
        let pump_err = tokio::spawn(pump_lines(
            stderr, LogStream::Stderr, log_tx.clone(), log_file.clone(),
        ));

        // Watcher 1: declare Connected once sing-box prints "started".
        let watcher = self.spawn_watcher();
        // Watcher 2: detect unexpected child exit and flip to Failed —
        //            this is the *only* path to Failed once spawn succeeded.
        let death_watcher = self.spawn_death_watcher();

        *self.inner.child.lock().await = Some(RunningChild {
            child,
            config_path,
            mode,
            pid: child_pid,
            pumps: vec![pump_out, pump_err, watcher, death_watcher],
        });

        // Snapshot what we just started with so the auto-restart loop can
        // reproduce it on its own. Cloning the JSON value once on connect
        // is cheap (~few KiB); avoids holding locks during restart.
        *self.inner.last_config.lock().await = Some(RestartContext {
            config: config.clone(),
            mode,
        });

        Ok(())
    }

    /// Stop sing-box. Idempotent across all entry paths:
    ///   * Disconnect button (this is "user-initiated" — disables
    ///     auto-restart for the rest of the session)
    ///   * Tauri shutdown handler
    ///   * Power-suspend handler
    ///   * RunEvent::Exit
    ///
    /// Sequence: mark `user_initiated_stop`, lock the child slot, send a
    /// graceful kill, wait up to 5 s, and if the child still hasn't gone
    /// away — escalate to a Win32 `TerminateProcess`. Pumps are aborted,
    /// the active config file is deleted, and if we were in TUN mode we
    /// hold for the wintun adapter teardown grace period before returning.
    /// Returns `Ok(())` even if there was nothing to stop — callers can
    /// invoke this any number of times.
    pub async fn stop(&self) -> CoreResult<()> {
        // Tell the death-watcher / auto-restart loop to stand down. This
        // must happen *before* we take the child out — otherwise a racing
        // death-watcher tick could observe the child gone and start the
        // restart loop while we're already on the shutdown path.
        self.inner.user_initiated_stop.store(true, Ordering::Release);
        // The next start() will reset both flags; we clear the stored
        // config now so a stale one isn't used by an in-flight restart
        // that might have read it before the flag flipped.
        *self.inner.last_config.lock().await = None;

        let mut guard = self.inner.child.lock().await;
        let Some(mut running) = guard.take() else {
            // Nothing to kill. Normalise state to Idle so a subsequent
            // start() doesn't observe a stale Failed/Stopping flag and
            // refuse for the wrong reason.
            drop(guard);
            if !matches!(*self.inner.state.read(), ConnectionState::Idle) {
                self.set_state(ConnectionState::Idle);
            }
            return Ok(());
        };
        drop(guard);

        self.set_state(ConnectionState::Stopping);

        // Step 1 — graceful kill via Tokio. On Windows this is
        // `TerminateProcess` on the child handle we own; on Unix it sends
        // SIGKILL. We rely on sing-box's atexit hooks for TUN cleanup.
        let _ = running.child.start_kill();
        let waited = tokio::time::timeout(Duration::from_secs(5), running.child.wait()).await;
        match waited {
            Ok(Ok(status)) => debug!(?status, "sing-box exited"),
            Ok(Err(e)) => warn!(?e, "wait failed"),
            Err(_) => {
                // Step 2 — force kill. Tokio's `kill()` may itself hang if
                // the child process is in an uninterruptible state, or if
                // we lost the handle for some reason. Drop straight to a
                // direct Win32 TerminateProcess by PID.
                warn!(pid = running.pid,
                    "sing-box did not exit in 5s, escalating to taskkill_force");
                if running.pid != 0 {
                    if let Err(e) = crate::process_guard::taskkill_force(running.pid) {
                        error!(target: "supervisor",
                            "taskkill_force({}) failed: {e}", running.pid);
                    }
                }
                // Even after the OS-level kill, drain Tokio's handle so
                // we don't leak a zombie pipe; bounded so we never block
                // indefinitely.
                let _ = tokio::time::timeout(Duration::from_secs(2), running.child.wait()).await;
            }
        }

        for h in running.pumps.drain(..) {
            h.abort();
        }
        if running.config_path.exists() {
            let _ = tokio::fs::remove_file(&running.config_path).await;
        }

        // Wintun grace period. The driver releases the virtual adapter
        // *asynchronously* — the kernel may take 400–800 ms even after
        // sing-box reports "exited". A reconnect within that window hits
        // "Cannot create a file when that file already exists" or, worse,
        // "Element not found". Block here so the next start has a clean
        // slate. 800 ms is the empirically-determined sweet spot — long
        // enough for the driver to settle, short enough to be invisible.
        if matches!(running.mode, ConnectionMode::Tun) {
            tokio::time::sleep(Duration::from_millis(800)).await;
        }

        self.set_state(ConnectionState::Idle);
        // Drop the secret only after we've fully torn down the child.
        // Anyone still racing on clash_api (a stale outbound_health
        // probe spawned moments ago) gets `None` and bails out cleanly
        // instead of mis-authenticating with a stale secret.
        *self.inner.clash_secret.write() = None;
        Ok(())
    }

    /// Internal restart trigger used by the auto-healing paths
    /// (death-watcher and state validator). NOT for user-initiated
    /// disconnects: it deliberately leaves `last_config` populated so the
    /// auto-restart loop has something to bring back. Pumps are aborted
    /// and the child is force-killed if needed; the caller is responsible
    /// for spawning the `auto_restart_loop` afterwards.
    async fn kill_for_restart(&self, reason: &str) {
        let mut guard = self.inner.child.lock().await;
        let Some(mut running) = guard.take() else { return };
        drop(guard);

        warn!(target: "supervisor", reason, "self-healing kill: terminating sing-box");
        let _ = running.child.start_kill();
        let waited = tokio::time::timeout(Duration::from_secs(3), running.child.wait()).await;
        if waited.is_err() && running.pid != 0 {
            let _ = crate::process_guard::taskkill_force(running.pid);
            let _ = tokio::time::timeout(Duration::from_secs(2), running.child.wait()).await;
        }
        for h in running.pumps.drain(..) {
            h.abort();
        }
        if running.config_path.exists() {
            let _ = tokio::fs::remove_file(&running.config_path).await;
        }
        if matches!(running.mode, ConnectionMode::Tun) {
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
    }

    /// Public API for the watchdog and the periodic state validator: ask
    /// the supervisor to kill the current child and bring it back up
    /// using the last known config. If `user_initiated_stop` is set
    /// (i.e. the user pressed Disconnect right before us), we no-op.
    /// Idempotent: a second concurrent call returns immediately because
    /// `auto_restart_in_flight` blocks duplicates.
    pub async fn request_self_heal(&self, reason: &'static str) {
        if self.inner.user_initiated_stop.load(Ordering::Acquire) {
            tracing::debug!(target: "supervisor", reason,
                "self-heal requested but user-initiated stop is set; ignoring");
            return;
        }
        self.kill_for_restart(reason).await;
        spawn_auto_restart_loop(self.inner.clone(), reason);
    }

    /* ----- internal ---------------------------------------------------- */

    fn set_state(&self, new_state: ConnectionState) {
        let mut s = self.inner.state.write();
        if *s != new_state {
            info!(from = ?*s, to = ?new_state, "state transition");
            *s = new_state.clone();
            drop(s);
            let _ = self.inner.state_tx.send(new_state);
        }
    }

    fn spawn_watcher(&self) -> JoinHandle<()> {
        let inner = self.inner.clone();
        let mut log_rx = inner.log_tx.subscribe();
        tokio::spawn(async move {
            // Watch for sing-box's "started" markers. We deliberately do NOT
            // flip to Failed on the substring "error"/"fatal": sing-box 1.13
            // emits ERROR-level deprecation warnings (e.g. legacy DNS) that
            // are recoverable. The authoritative Failed signal comes from
            // the death-watcher detecting an actual process exit.
            while let Ok(line) = log_rx.recv().await {
                let lower = line.text.to_ascii_lowercase();
                if lower.contains("sing-box started")
                    || lower.contains("started inbound/")
                    || lower.contains("started service")
                {
                    let mut s = inner.state.write();
                    if matches!(*s, ConnectionState::Starting) {
                        *s = ConnectionState::Connected;
                        let _ = inner.state_tx.send(ConnectionState::Connected);
                    }
                }
            }
        })
    }

    /// Watch the spawned child for an unexpected exit. Records the last few
    /// stderr lines as the failure reason. This is the *only* path that
    /// flips state to `Failed` once the child is up.
    fn spawn_death_watcher(&self) -> JoinHandle<()> {
        let inner = self.inner.clone();
        let mut log_rx = inner.log_tx.subscribe();
        tokio::spawn(async move {
            let mut tail: std::collections::VecDeque<String> =
                std::collections::VecDeque::new();
            const TAIL_MAX: usize = 6;

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                        let mut guard = inner.child.lock().await;
                        let exited_status = match guard.as_mut() {
                            Some(rc) => match rc.child.try_wait() {
                                Ok(None)         => None,
                                Ok(Some(status)) => Some(Ok(status)),
                                Err(e)           => Some(Err(e)),
                            },
                            None => return, // someone else already cleared it
                        };

                        match exited_status {
                            None => { /* still running */ }
                            Some(Err(e)) => {
                                tracing::warn!(target: "supervisor", "try_wait error: {e}");
                            }
                            Some(Ok(status)) => {
                                let was_intentional = matches!(
                                    *inner.state.read(),
                                    ConnectionState::Stopping | ConnectionState::Idle
                                ) || inner.user_initiated_stop.load(Ordering::Acquire);
                                // Drain the slot so the next start() doesn't
                                // see a corpse. This is the critical fix for
                                // the "sing-box already running" wedge after
                                // an unexpected exit.
                                let mut taken = guard.take().expect("just matched Some");
                                drop(guard);
                                for h in taken.pumps.drain(..) { h.abort(); }
                                if taken.config_path.exists() {
                                    let _ = tokio::fs::remove_file(&taken.config_path).await;
                                }
                                if !was_intentional {
                                    let reason = if tail.is_empty() {
                                        format!("sing-box exited unexpectedly: {status}")
                                    } else {
                                        format!(
                                            "sing-box exited ({status}): {}",
                                            tail.iter().cloned().collect::<Vec<_>>().join(" | ")
                                        )
                                    };
                                    tracing::error!(target: "supervisor", "{reason}");
                                    let mut s = inner.state.write();
                                    *s = ConnectionState::Failed { reason: reason.clone() };
                                    drop(s);
                                    let _ = inner.state_tx.send(ConnectionState::Failed { reason });

                                    // Self-heal: bring sing-box back up
                                    // automatically. Only fires for
                                    // unexpected deaths; explicit
                                    // disconnects flip user_initiated_stop
                                    // and short-circuit the branch above.
                                    spawn_auto_restart_loop(inner.clone(),
                                        "death-watcher: child exited unexpectedly");
                                }
                                // TUN grace period — let wintun release the
                                // adapter before we let any subsequent
                                // start() race in.
                                if matches!(taken.mode, ConnectionMode::Tun) {
                                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                                }
                                return;
                            }
                        }
                    }
                    line = log_rx.recv() => {
                        if let Ok(l) = line {
                            if matches!(l.stream, LogStream::Stderr) {
                                if tail.len() >= TAIL_MAX { tail.pop_front(); }
                                tail.push_back(l.text);
                            }
                        }
                    }
                }
            }
        })
    }
}

/// Backoff schedule for the auto-restart loop, in seconds. Tuned to make
/// transient failures (network blip, transient REALITY handshake failure,
/// brief NIC reset) heal in seconds, while sustained failures back off
/// fast enough to not pin the CPU or burn through clash_api retries. After
/// the last entry the loop gives up and leaves the supervisor in `Failed`
/// for the user to handle manually.
const AUTO_RESTART_BACKOFF_SECONDS: &[u64] = &[1, 2, 5, 15, 30, 60, 120];

/// Spawn a Tokio task that re-runs `start()` with the last known config
/// using the schedule above. Idempotent via
/// `inner.auto_restart_in_flight`: a second concurrent call is a no-op.
fn spawn_auto_restart_loop(inner: Arc<SupervisorInner>, reason: &'static str) {
    // Compare-and-swap so only the first caller wins. If we lose, another
    // task is already on the case and will pick up the latest config from
    // the mutex on its next iteration.
    if inner
        .auto_restart_in_flight
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!(target: "supervisor",
            "auto-restart already in flight, skipping duplicate trigger ({reason})");
        return;
    }

    tracing::warn!(target: "supervisor",
        "scheduling auto-restart (reason: {reason})");

    tokio::spawn(async move {
        // Reconstruct a borrowed Supervisor handle from the inner Arc.
        // We can't capture `&self` because the death-watcher path didn't
        // hold one — `Supervisor::clone()` is cheap (Arc bump).
        let supervisor = Supervisor { inner: inner.clone() };

        for (attempt, secs) in AUTO_RESTART_BACKOFF_SECONDS.iter().enumerate() {
            // Honour user disconnect at every step.
            if inner.user_initiated_stop.load(Ordering::Acquire) {
                tracing::info!(target: "supervisor",
                    "auto-restart aborted: user-initiated stop");
                break;
            }
            tracing::info!(target: "supervisor",
                "auto-restart attempt {} of {} in {}s",
                attempt + 1,
                AUTO_RESTART_BACKOFF_SECONDS.len(),
                secs);
            tokio::time::sleep(Duration::from_secs(*secs)).await;
            if inner.user_initiated_stop.load(Ordering::Acquire) {
                break;
            }

            // Snapshot the last config (cloned so we don't hold the lock
            // across the start() await — start() itself takes the same
            // mutex via last_config.lock() to repopulate).
            let ctx = match inner.last_config.lock().await.clone() {
                Some(c) => c,
                None => {
                    tracing::warn!(target: "supervisor",
                        "auto-restart: no last_config recorded, giving up");
                    break;
                }
            };

            match supervisor.start(&ctx.config, ctx.mode).await {
                Ok(()) => {
                    tracing::info!(target: "supervisor",
                        "auto-restart succeeded on attempt {}", attempt + 1);
                    // Sing-box is up. The state validator (if running)
                    // will keep an eye on it; our job here is done.
                    break;
                }
                Err(e) => {
                    tracing::warn!(target: "supervisor",
                        "auto-restart attempt {} failed: {e}", attempt + 1);
                    // Continue to the next backoff bucket.
                }
            }
        }

        inner
            .auto_restart_in_flight
            .store(false, Ordering::Release);
    });
}

async fn pump_lines<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    stream: LogStream,
    tx: broadcast::Sender<LogLine>,
    file: std::sync::Arc<std::sync::Mutex<Option<std::fs::File>>>,
) {
    use std::io::Write;
    let mut reader = BufReader::new(reader).lines();
    loop {
        match reader.next_line().await {
            Ok(Some(line)) => {
                let line = strip_ansi(&line);
                let line = sanitise_log_line(&line);
                // 1. Mirror to dedicated sing-box.log file.
                if let Ok(mut guard) = file.lock() {
                    if let Some(f) = guard.as_mut() {
                        let _ = writeln!(
                            f,
                            "[{:5}] {}",
                            match stream {
                                LogStream::Stdout => "out",
                                LogStream::Stderr => "err",
                            },
                            line
                        );
                    }
                }
                // 2. Echo into the unified tracing pipeline. FATAL → error,
                //    ERROR → warn, anything else → debug. This lets the
                //    main `v2pn.log` carry the actual cause of any startup
                //    failure without us hunting across two files.
                let lower = line.to_ascii_lowercase();
                if lower.contains("fatal") {
                    tracing::error!(target: "singbox", ?stream, "{line}");
                } else if lower.contains("error") {
                    tracing::warn!(target: "singbox", ?stream, "{line}");
                } else {
                    tracing::debug!(target: "singbox", ?stream, "{line}");
                }
                // 3. Broadcast to UI subscribers (Logs view).
                let _ = tx.send(LogLine { stream, text: line });
            }
            Ok(None) => break,
            Err(e) => {
                error!(?e, ?stream, "pump_lines read failed");
                break;
            }
        }
    }
}

/// Strip ANSI/VT100 escape sequences (sing-box prints colour codes on stderr
/// even when piped). Without this they reach the UI as literal `[31m...[0m`.
fn strip_ansi(line: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static ANSI_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\x1b\[[0-9;]*[mK]|\[[0-9;]+m").unwrap());
    ANSI_RE.replace_all(line, "").into_owned()
}

/// Mask UUIDs and 32+ char hex blobs so we don't leak credentials in the UI/log file.
fn sanitise_log_line(line: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static UUID_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap()
    });
    UUID_RE.replace_all(line, "<uuid>").into_owned()
}

/// Best-effort path resolution for the bundled sing-box binary.
///
/// Search order:
///  1. `V2PN_SINGBOX` env (used by `tauri dev`)
///  2. sibling of the host process: `<exe_dir>/sing-box(.exe)?`
///  3. on Windows specifically: `<exe_dir>/sing-box-x86_64-pc-windows-msvc.exe`
///     (Tauri's externalBin naming convention before bundling)
///  4. `<repo>/crates/tauri-app/binaries/sing-box-x86_64-pc-windows-msvc.exe`
///     (developer fallback)
pub fn resolve_singbox_binary(exe_dir: &Path) -> Option<PathBuf> {
    if let Ok(env_path) = std::env::var("V2PN_SINGBOX") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }

    let candidates = [
        exe_dir.join(if cfg!(windows) { "sing-box.exe" } else { "sing-box" }),
        exe_dir.join("sing-box-x86_64-pc-windows-msvc.exe"),
        exe_dir.join("sing-box-x86_64-pc-windows-msvc"),
        // dev fallback: walk up to repo root
        exe_dir
            .ancestors()
            .nth(3)
            .map(|p| p.join("crates/tauri-app/binaries/sing-box-x86_64-pc-windows-msvc.exe"))
            .unwrap_or_default(),
    ];

    candidates.into_iter().find(|p| p.exists())
}
