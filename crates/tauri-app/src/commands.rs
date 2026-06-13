use std::path::PathBuf;
use std::sync::Arc;

use app_core::profile::ProxyProfile;
use app_core::singbox::{
    config::{ConnectionMode, ConnectionOptions},
    sanitize::sanitize_strict,
};
use app_core::state_guard::{recover_orphaned_state, ConnectionGuard, RecoveryOutcome};
use app_core::subscription::fetch::{fetch_subscription, parse_body, FetchOptions};
use app_core::subscription::ParsedSubscription;
use app_core::supervisor::{ConnectionState, Supervisor};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        CommandError { message: e.to_string() }
    }
}

/// Shared application state held by Tauri.
/// What we need to restart a connection without the user touching the UI.
/// Captured at the end of every successful `connect_inner` so background
/// systems (suspend/resume handler, watchdog auto-restart) can replay it.
#[derive(Clone)]
pub struct LastConnectIntent {
    pub profiles: Vec<ProxyProfile>,
    pub selected_id: String,
    pub mode: ConnectionMode,
}

pub struct AppState {
    pub supervisor: Arc<Supervisor>,
    pub options: Arc<Mutex<ConnectionOptions>>,
    /// Active proxy guard. `None` while disconnected.
    pub guard: Arc<Mutex<Option<ConnectionGuard>>>,
    /// Profile id currently routed by the running sing-box (`None` while idle).
    /// Used by the frontend to know whether `switch_server` is applicable.
    pub active_selected: Arc<Mutex<Option<String>>>,
    /// Snapshot of what the user last connected to. Used by the
    /// suspend/resume handler to bring the connection back automatically
    /// after the laptop wakes up. Cleared by an explicit `disconnect`.
    pub last_intent: Arc<Mutex<Option<LastConnectIntent>>>,
    /// Was the user connected when the system started suspending? Set in
    /// the `Suspend` branch of the power handler, consumed in `Resume`
    /// to decide whether to auto-reconnect or stay idle.
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

/* ============================================================ subscription */

#[tauri::command]
pub async fn subscription_fetch(url: String) -> Result<ParsedSubscription, CommandError> {
    let started = std::time::Instant::now();
    tracing::info!(target: "v2pn::cmd", url = %url, "subscription_fetch begin");
    let opts = FetchOptions::default();
    let res = fetch_subscription(&url, &opts).await;
    match &res {
        Ok(parsed) => tracing::info!(
            target: "v2pn::cmd",
            elapsed_ms = started.elapsed().as_millis() as u64,
            profiles = parsed.profiles.len(),
            title = ?parsed.meta.title,
            total_bytes = ?parsed.meta.total_bytes,
            expire_at = ?parsed.meta.expire_at,
            "subscription_fetch ok"
        ),
        Err(e) => tracing::warn!(
            target: "v2pn::cmd",
            elapsed_ms = started.elapsed().as_millis() as u64,
            error = %e,
            "subscription_fetch failed"
        ),
    }
    Ok(res?)
}

#[tauri::command]
pub async fn subscription_parse_text(text: String) -> Result<ParsedSubscription, CommandError> {
    tracing::info!(target: "v2pn::cmd", bytes = text.len(), "subscription_parse_text");
    let profiles = parse_body(text.as_bytes())?;
    Ok(ParsedSubscription { profiles, meta: Default::default() })
}

#[tauri::command]
pub async fn subscription_parse_uri(uri: String) -> Result<ProxyProfile, CommandError> {
    tracing::info!(target: "v2pn::cmd", uri = %uri, "subscription_parse_uri");
    Ok(app_core::subscription::uri::parse_uri(&uri)?)
}

/* ============================================================ connection */

#[tauri::command]
pub async fn connect(
    profile: ProxyProfile,
    mode: Option<ConnectionMode>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    connect_inner(vec![profile.clone()], profile.id, mode, state, app).await
}

/// Multi-profile connect: starts sing-box with **all** servers from the
/// active subscription wired up to a `selector` outbound. Subsequent server
/// changes go through `switch_server` and don't restart the engine.
#[tauri::command]
pub async fn connect_subscription(
    profiles: Vec<ProxyProfile>,
    selected_id: String,
    mode: Option<ConnectionMode>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    if profiles.is_empty() {
        return Err(CommandError {
            message: "no profiles to connect".into(),
        });
    }
    connect_inner(profiles, selected_id, mode, state, app).await
}

async fn connect_inner(
    profiles: Vec<ProxyProfile>,
    selected_id: String,
    mode: Option<ConnectionMode>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    // Forward into the internal version. Both the Tauri command path and
    // the power-handler resume path want exactly the same logic; the only
    // difference is that resume can't carry a Tauri `State<'_, AppState>`
    // borrow across the suspend boundary, so we need a `&AppState`-friendly
    // entrypoint.
    connect_inner_with_state(profiles, selected_id, mode, &state, app).await
}

/// Reusable connect path. Used by `connect`, `connect_subscription`, and
/// the suspend/resume auto-reconnect handler. The `&AppState` form makes
/// it callable from a spawned task that just has an `AppHandle`.
pub async fn connect_subscription_internal(
    profiles: Vec<ProxyProfile>,
    selected_id: String,
    mode: Option<ConnectionMode>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    if profiles.is_empty() {
        return Err(CommandError {
            message: "no profiles to connect".into(),
        });
    }
    connect_inner_with_state(profiles, selected_id, mode, &state, app).await
}

async fn connect_inner_with_state(
    profiles: Vec<ProxyProfile>,
    selected_id: String,
    mode: Option<ConnectionMode>,
    state: &AppState,
    app: AppHandle,
) -> Result<(), CommandError> {
    let started = std::time::Instant::now();
    if state.guard.lock().await.is_some() {
        tracing::warn!(target: "v2pn::connect", "rejected: already connected");
        return Err(CommandError {
            message: "already connected; call disconnect first".into(),
        });
    }

    let mut opts = state.options.lock().await.clone();
    if let Some(m) = mode {
        opts.mode = m;
    }

    // Port pre-flight: if 7890/9090 are taken by another process (a stale
    // sing-box we couldn't kill, a rival Clash client, a corp proxy),
    // walk forward to the first pair we can actually bind. Sing-box gets
    // to grab them on the next cycle. Without this the connect would
    // fail with a cryptic "address in use" deep in the engine log.
    let preferred_mixed = opts.mixed_port;
    let preferred_clash = opts.clash_api_port;
    opts.mixed_port = app_core::port_pick::pick_free_port(preferred_mixed);
    opts.clash_api_port = app_core::port_pick::pick_free_port(preferred_clash);
    if opts.mixed_port != preferred_mixed {
        tracing::warn!(target: "v2pn::connect",
            "mixed_port {preferred_mixed} occupied → using {} instead",
            opts.mixed_port);
    }
    if opts.clash_api_port != preferred_clash {
        tracing::warn!(target: "v2pn::connect",
            "clash_api_port {preferred_clash} occupied → using {} instead",
            opts.clash_api_port);
    }

    tracing::info!(
        target: "v2pn::connect",
        profiles = profiles.len(),
        selected = %selected_id,
        mode = ?opts.mode,
        mixed_port = opts.mixed_port,
        clash_port = opts.clash_api_port,
        ipv6 = opts.ipv6,
        strict_dns = opts.strict_dns,
        "begin"
    );

    let mut cfg = app_core::singbox::config::build_config_multi(&profiles, &selected_id, &opts);
    let report = sanitize_strict(&mut cfg).map_err(|e| CommandError {
        message: format!("config rejected by sanitiser: {e}"),
    })?;
    for w in report.warnings {
        tracing::warn!(target: "v2pn::sanitize", "{w}");
    }

    // Inject a fresh random clash_api secret AFTER sanitize. The
    // sanitiser strips any attacker-supplied secret from the untrusted
    // subscription; this rotation gives us our own private token. Without
    // it, any other process on the machine (browser tab making fetch()
    // to localhost, malicious tooling, even a legit but compromised app)
    // could PUT /proxies/proxy and steal/redirect traffic.
    let _clash_secret = state.supervisor.rotate_clash_secret(&mut cfg);

    tracing::trace!(target: "v2pn::connect", "sanitised config ready");

    let bypass = ["localhost", "127.*", "10.*", "172.16.*", "192.168.*", "<local>"];
    let guard = match opts.mode {
        ConnectionMode::Proxy => {
            let addr = format!("127.0.0.1:{}", opts.mixed_port);
            tracing::debug!(target: "v2pn::connect", addr = %addr, "acquiring proxy guard");
            ConnectionGuard::acquire_proxy(&state.state_dir, &addr, &bypass)?
        }
        ConnectionMode::Tun => {
            // Best-effort: clear any stale Wintun adapter from a previous
            // crashed run before we ask sing-box to create a new one.
            // Without this the kernel can hold the adapter for several
            // seconds after sing-box dies hard, and the next start fails
            // with "Cannot create a file when that file already exists".
            app_core::wintun_cleanup::cleanup_stale_adapter(&opts.tun_interface_name);
            tracing::debug!(target: "v2pn::connect", "acquiring tun guard");
            ConnectionGuard::acquire_tun(&state.state_dir)?
        }
    };

    if let Err(e) = state.supervisor.start(&cfg, opts.mode).await {
        tracing::error!(target: "v2pn::connect", error = %e, "supervisor.start failed");
        drop(guard);
        return Err(CommandError {
            message: format!("supervisor.start failed: {e}"),
        });
    }

    // Stamp the spawned sing-box PID into the on-disk state file so that
    // `recover_orphaned_state` on the next launch can force-kill any
    // orphan even if our Job Object guarantee somehow didn't fire (very
    // rare — usually only on pre-creation failure or admin/non-admin
    // boundary issues). Failing to write the PID is non-fatal: it just
    // means a future recovery falls back to the process-table scan.
    let mut guard = guard;
    if let Some(pid) = state.supervisor.child_pid().await {
        if let Err(e) = guard.update_child_pid(pid) {
            tracing::warn!(target: "v2pn::connect",
                "could not record child_pid={pid} into state file: {e}");
        }
    }

    *state.guard.lock().await = Some(guard);
    *state.options.lock().await = opts.clone();
    *state.active_selected.lock().await = Some(selected_id.clone());

    // Snapshot what the user just asked for so the suspend/resume handler
    // (and any future "Reconnect" hotkey) can replay it without going
    // through the UI again.
    *state.last_intent.lock().await = Some(LastConnectIntent {
        profiles: profiles.clone(),
        selected_id: selected_id.clone(),
        mode: opts.mode,
    });

    let _ = app.emit("connection-state", state.supervisor.state());
    tracing::info!(
        target: "v2pn::connect",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "command returned (engine still starting)"
    );
    Ok(())
}

/// Hot-switch the active server within an already-running sing-box. Does
/// **not** restart the engine, doesn't touch the TUN adapter, doesn't
/// re-apply the OS proxy. Returns ~10 ms in the happy path.
#[tauri::command]
pub async fn switch_server(
    profile_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    let opts = state.options.lock().await.clone();
    let tag = app_core::singbox::config::server_tag_for(&profile_id);
    let secret = state.supervisor.clash_secret();
    let url = format!(
        "http://127.0.0.1:{}/proxies/proxy",
        opts.clash_api_port
    );

    tracing::info!(target: "v2pn::switch", "PUT {url} body={{name:'{tag}'}}");

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(2500))
        .build()
        .map_err(|e| CommandError { message: format!("client: {e}") })?;

    let mut req = client.put(&url).json(&serde_json::json!({ "name": tag }));
    if let Some(ref s) = secret {
        req = req.bearer_auth(s);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(target: "v2pn::switch", "request failed: {e}");
            CommandError { message: format!("clash_api: {e}") }
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(target: "v2pn::switch", "clash_api {} body={}", status.as_u16(), body);
        return Err(CommandError {
            message: format!("clash_api {} — {body}", status.as_u16()),
        });
    }

    tracing::info!(target: "v2pn::switch", "ok → tag={tag}");

    // Fire-and-forget health probe so the UI knows whether the new server
    // is reachable. Result emitted as `outbound-health` event (consumed by
    // the connection store / status badge). We don't `.await` it here:
    // switch_server should remain ~instant from the user's perspective —
    // the probe takes up to ~5s, and waiting on it would defeat the point
    // of hot-switching.
    {
        let app = app.clone();
        let tag = tag.clone();
        let port = opts.clash_api_port;
        let pid = profile_id.clone();
        let secret = secret.clone();
        tokio::spawn(async move {
            let h = app_core::outbound_health::probe(port, &tag, secret.as_deref()).await;
            tracing::info!(
                target: "v2pn::health",
                profile = %pid,
                tag = %tag,
                latency_ms = ?h.latency_ms,
                error = ?h.error,
                "post-switch probe"
            );
            let _ = app.emit("outbound-health", &h);
        });
    }

    // Force-close every active connection so old TCP sessions don't keep
    // streaming through the previous outbound. `interrupt_exist_connections`
    // on the selector does this only for sessions sing-box owns at the
    // application layer — gvisor-stack TUN sessions sit a layer below and
    // need an explicit wipe via the clash API.
    let close_url = format!(
        "http://127.0.0.1:{}/connections",
        opts.clash_api_port
    );
    let mut del_req = client.delete(&close_url);
    if let Some(ref s) = secret {
        del_req = del_req.bearer_auth(s);
    }
    match del_req.send().await {
        Ok(resp) => {
            tracing::info!(target: "v2pn::switch",
                "DELETE /connections → {}", resp.status().as_u16());
        }
        Err(e) => {
            tracing::warn!(target: "v2pn::switch",
                "DELETE /connections failed (non-fatal): {e}");
        }
    }

    *state.active_selected.lock().await = Some(profile_id);
    Ok(())
}

#[tauri::command]
pub async fn disconnect(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    state.supervisor.stop().await?;
    if let Some(guard) = state.guard.lock().await.take() {
        guard.release()?;
    }
    *state.active_selected.lock().await = None;
    // Explicit user disconnect — wipe the resume intent so a later
    // wake-from-sleep doesn't bring the connection back without consent.
    *state.last_intent.lock().await = None;
    *state.suspend_was_connected.lock().await = false;
    let _ = app.emit("connection-state", state.supervisor.state());
    Ok(())
}

#[tauri::command]
pub async fn active_server_id(
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    Ok(state.active_selected.lock().await.clone())
}

#[tauri::command]
pub async fn connection_state(state: State<'_, AppState>) -> Result<ConnectionState, CommandError> {
    Ok(state.supervisor.state())
}

#[tauri::command]
pub async fn set_connection_mode(
    mode: ConnectionMode,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.options.lock().await.mode = mode;
    Ok(())
}

#[tauri::command]
pub async fn get_connection_options(
    state: State<'_, AppState>,
) -> Result<ConnectionOptions, CommandError> {
    Ok(state.options.lock().await.clone())
}

/* ============================================================ misc */

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
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

/// Self-diagnostic snapshot — useful for bug reports. Includes versions,
/// elevation, current connection state, port usage, and the last connection
/// log lines (already mirrored to disk).
#[tauri::command]
pub async fn diagnostics(state: State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
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
    if cfg!(target_os = "windows") { "windows" }
    else if cfg!(target_os = "macos") { "macos" }
    else if cfg!(target_os = "linux") { "linux" }
    else { "unknown" }
}

/* ============================================================ probes */

#[tauri::command]
pub async fn probe_latency_batch(
    profiles: Vec<ProxyProfile>,
) -> Result<Vec<app_core::probe::PingResult>, CommandError> {
    let targets = profiles
        .into_iter()
        .map(|p| (p.id, p.server, p.port))
        .collect();
    Ok(app_core::probe::probe_many(targets).await)
}

/* ============================================================ elevation */

#[tauri::command]
pub fn elevation_status() -> app_core::elevation::ElevationStatus {
    app_core::elevation::is_elevated()
}

/// Re-launch the current process via Windows UAC. On success, this exits the
/// caller — the elevated copy takes over.
#[tauri::command]
pub async fn restart_as_admin(app: AppHandle) -> Result<(), CommandError> {
    // Stop sing-box and release the OS proxy *before* spawning the elevated
    // copy, so we don't leave the system in an in-between state.
    if let Some(state) = app.try_state::<AppState>() {
        let _ = state.supervisor.stop().await;
        let taken = { state.guard.lock().await.take() };
        if let Some(g) = taken {
            let _ = g.release();
        }
    }
    app_core::elevation::restart_as_admin()
        .map_err(|e| CommandError { message: format!("UAC: {e}") })?;
    // Give the new process a moment to claim the foreground.
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    app.exit(0);
    Ok(())
}

/* ============================================================ event bridge */

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

/* ============================================================ recovery */

/// Run startup recovery. Logs the outcome and returns it for the UI to display.
pub fn run_startup_recovery(state_dir: &std::path::Path) -> RecoveryOutcome {
    let runtime_dir = state_dir;

    // Step 1: state-file recovery. This handles the standard case where
    // the previous v2pn left a guard file behind, restores the OS proxy,
    // and force-kills the recorded child PID.
    let outcome = match recover_orphaned_state(state_dir) {
        Ok(outcome) => {
            match &outcome {
                RecoveryOutcome::NothingToDo => {}
                RecoveryOutcome::OwnedByLiveProcess { pid } => {
                    tracing::warn!(target: "recovery",
                        "another v2pn (PID {pid}) holds the proxy; not touching");
                }
                RecoveryOutcome::Recovered { applied, .. } => {
                    tracing::info!(target: "recovery",
                        "recovered from previous crash, restored proxy (was {:?})", applied);
                }
            }
            outcome
        }
        Err(e) => {
            tracing::error!(target: "recovery", "recovery failed: {e}");
            RecoveryOutcome::NothingToDo
        }
    };

    // Step 2: orphan process scan. Independent fallback for the cases
    // where the state file was deleted by hand, the previous v2pn never
    // got a chance to write it, or the kill-on-close Job Object didn't
    // fire. Walks the process table, kills any sing-box.exe whose -D
    // argument points at our runtime_dir. Skipped if Step 1 says another
    // v2pn instance is the legitimate owner — in that case the foreign
    // sing-box belongs to it, not to us.
    if !matches!(outcome, RecoveryOutcome::OwnedByLiveProcess { .. }) {
        let killed = app_core::process_guard::kill_orphan_singboxes_for_runtime(runtime_dir);
        if killed > 0 {
            // Wintun grace period: the kernel queues adapter teardown for
            // a few hundred ms after the holding process exits. Without
            // this sleep the next supervisor::start() may race the cleanup
            // and bail with "Element not found" / "Cannot create a file".
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
    }

    outcome
}
