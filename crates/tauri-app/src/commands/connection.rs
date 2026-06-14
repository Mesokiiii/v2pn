//! Connection lifecycle: connect / connect_subscription / switch_server
//! / disconnect / state queries, plus the shared `shutdown_session` and
//! `release_guard_after_failure` helpers used by every "tear it down"
//! call-site (Tauri command, tray menu, power suspend, RunEvent::Exit,
//! Failed-state auto-cleanup).

use app_core::profile::ProxyProfile;
use app_core::singbox::config::ConnectionMode;
use app_core::singbox::sanitize::sanitize_strict;
use app_core::state_guard::ConnectionGuard;
use app_core::supervisor::ConnectionState;
use tauri::{AppHandle, Emitter, State};

use super::{AppState, CommandError, LastConnectIntent};

/* ----- connect / switch -------------------------------------------------- */

#[tauri::command]
pub async fn connect(
    profile: ProxyProfile,
    mode: Option<ConnectionMode>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    connect_inner_with_state(vec![profile.clone()], profile.id, mode, &state, app).await
}

/// Multi-profile connect: starts sing-box with **every** server from
/// the active subscription wired up to a `selector` outbound. Subsequent
/// server changes go through `switch_server` and don't restart sing-box.
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
    connect_inner_with_state(profiles, selected_id, mode, &state, app).await
}

/// Reusable connect path. Used by `connect`, `connect_subscription`,
/// and the suspend/resume auto-reconnect handler. The `&AppState` form
/// makes it callable from a spawned task that just has an `AppHandle`
/// — the suspend boundary won't carry a Tauri `State<'_>` borrow.
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

    // Port pre-flight: if 7890/9090 are taken by another process (a
    // stale sing-box we couldn't kill, a rival Clash client, a corp
    // proxy), walk forward to the first pair we can actually bind.
    // Sing-box gets to grab them on the next cycle. Without this the
    // connect fails with a cryptic "address in use" deep in the engine
    // log.
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

    let mut cfg =
        app_core::singbox::config::build_config_multi(&profiles, &selected_id, &opts);
    let report = sanitize_strict(&mut cfg).map_err(|e| CommandError {
        message: format!("config rejected by sanitiser: {e}"),
    })?;
    for w in report.warnings {
        tracing::warn!(target: "v2pn::sanitize", "{w}");
    }

    // Inject a fresh random clash_api secret AFTER sanitize. The
    // sanitiser strips any attacker-supplied secret from the untrusted
    // subscription; this rotation gives us our own private token.
    // Without it, any other process on the machine could PUT
    // /proxies/proxy and steal/redirect traffic.
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
            // Best-effort: clear any stale Wintun adapter from a
            // previous crashed run before we ask sing-box to create a
            // new one. Without this the kernel can hold the adapter
            // for several seconds after sing-box dies hard, and the
            // next start fails with "Cannot create a file when that
            // file already exists".
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

    // Stamp the spawned sing-box PID into the on-disk state file so
    // recovery on the next launch can force-kill an orphan even if our
    // Job Object guarantee somehow didn't fire. Failing to write the
    // PID is non-fatal — recovery falls back to the process-table scan.
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

    // Snapshot what the user just asked for so the suspend/resume
    // handler (and any future "Reconnect" hotkey) can replay it
    // without going through the UI again.
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

/// Hot-switch the active server within an already-running sing-box.
/// Does **not** restart the engine, doesn't touch the TUN adapter,
/// doesn't re-apply the OS proxy. Returns ~10 ms in the happy path.
#[tauri::command]
pub async fn switch_server(
    profile_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    let opts = state.options.lock().await.clone();
    let tag = app_core::singbox::config::server_tag_for(&profile_id);
    let secret = state.supervisor.clash_secret();
    let port = opts.clash_api_port;

    let client = app_core::clash_api::Client::new(port, secret.clone()).map_err(|e| {
        CommandError { message: format!("clash_api client: {e}") }
    })?;

    tracing::info!(target: "v2pn::switch", port, %tag, "switching outbound");
    if let Err(e) = client.switch_outbound(&tag).await {
        tracing::warn!(target: "v2pn::switch", "switch failed: {e}");
        return Err(CommandError { message: e.to_string() });
    }
    tracing::info!(target: "v2pn::switch", "ok → tag={tag}");

    // Fire-and-forget health probe so the UI knows whether the new
    // server is actually reachable through the full proxy stack.
    // Detached so switch_server stays ~instant from the user's POV.
    {
        let app = app.clone();
        let tag = tag.clone();
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

    // Force-close every active connection so old TCP sessions don't
    // keep streaming through the previous outbound.
    // `interrupt_exist_connections` on the selector handles only what
    // sing-box owns at the application layer; gvisor-stack TUN
    // sessions sit a layer below and need this explicit wipe.
    match client.close_all_connections().await {
        Ok(resp) => tracing::info!(target: "v2pn::switch",
            "DELETE /connections → {}", resp.status().as_u16()),
        Err(e) => tracing::warn!(target: "v2pn::switch",
            "DELETE /connections failed (non-fatal): {e}"),
    }

    *state.active_selected.lock().await = Some(profile_id);
    Ok(())
}

/* ----- shutdown helpers -------------------------------------------------- */

/// Selector flags for [`shutdown_session`] — different shutdown
/// call-sites want slightly different sub-sets of the cleanup. Encoded
/// as named fields so the call-site reads as English ("shutdown but
/// keep last_intent so resume can replay it").
#[derive(Debug, Clone, Copy)]
pub struct ShutdownOpts {
    /// Wipe `last_intent`. False on power-Suspend (we WANT resume to
    /// replay), true on user-initiated disconnects.
    pub clear_intent: bool,
    /// Reset `suspend_was_connected`. Mirror of `clear_intent` for
    /// almost every call-site; broken out so the resume-replay handler
    /// can flip just this one without touching the intent.
    pub clear_suspend_flag: bool,
    /// Emit a `connection-state` Tauri event after the session is
    /// down. False on RunEvent::Exit (the webview is already gone).
    pub emit_state_event: bool,
}

impl ShutdownOpts {
    /// User pressed Disconnect (or the tray menu equivalent). Wipes
    /// resume intent so a later wake doesn't auto-reconnect against
    /// the user's stated wish.
    pub const USER_DISCONNECT: Self = Self {
        clear_intent: true,
        clear_suspend_flag: true,
        emit_state_event: true,
    };
    /// System is going to sleep. Keep the intent so `Resume` can
    /// replay it; the suspend handler flips its own bit separately.
    pub const SUSPEND: Self = Self {
        clear_intent: false,
        clear_suspend_flag: false,
        emit_state_event: true,
    };
    /// Process is about to exit (Quit menu / RunEvent::Exit). No event
    /// to emit — the receiver is dead anyway.
    pub const PROCESS_EXIT: Self = Self {
        clear_intent: true,
        clear_suspend_flag: true,
        emit_state_event: false,
    };
}

/// One shutdown sequence to rule them all. Stops the sing-box engine,
/// drops the connection guard (which restores the OS proxy via Drop),
/// then optionally clears the active server / resume intent / suspend
/// flag and emits a connection-state event.
///
/// Replaces the 5 sites that used to spell this out by hand:
///   * [`disconnect`] command (user pressed the button)
///   * Tray menu "Disconnect" handler
///   * Tray menu "Quit" handler
///   * Power Suspend handler
///   * `RunEvent::Exit` final cleanup
///
/// Errors from `supervisor.stop` / `guard.release` are swallowed: every
/// caller is on a path where the user's expectation is "tear it down,
/// best effort". Surfacing one of these errors back through Tauri just
/// produces toasts that don't help the user.
pub async fn shutdown_session(state: &AppState, app: &AppHandle, opts: ShutdownOpts) {
    if let Err(e) = state.supervisor.stop().await {
        tracing::warn!(target: "v2pn::shutdown", "supervisor.stop: {e}");
    }
    let taken = { state.guard.lock().await.take() };
    if let Some(g) = taken {
        if let Err(e) = g.release() {
            tracing::warn!(target: "v2pn::shutdown", "guard.release: {e}");
        }
    }
    *state.active_selected.lock().await = None;
    if opts.clear_intent {
        *state.last_intent.lock().await = None;
    }
    if opts.clear_suspend_flag {
        *state.suspend_was_connected.lock().await = false;
    }
    if opts.emit_state_event {
        let _ = app.emit("connection-state", state.supervisor.state());
    }
}

/// Lighter-weight cousin of [`shutdown_session`] for the Failed-state
/// auto-cleanup path: sing-box has already died on its own, so calling
/// `supervisor.stop()` would be a no-op. We just need to release the
/// guard (which restores the OS proxy) and clear the active server
/// pointer so the UI doesn't claim to be routed to a dead engine.
pub async fn release_guard_after_failure(state: &AppState) {
    let taken = { state.guard.lock().await.take() };
    if let Some(g) = taken {
        if let Err(e) = g.release() {
            tracing::warn!(target: "v2pn::shutdown",
                "release_guard_after_failure: {e}");
        }
    }
    *state.active_selected.lock().await = None;
}

/* ----- disconnect + state queries --------------------------------------- */

#[tauri::command]
pub async fn disconnect(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    shutdown_session(&state, &app, ShutdownOpts::USER_DISCONNECT).await;
    Ok(())
}

#[tauri::command]
pub async fn active_server_id(
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    Ok(state.active_selected.lock().await.clone())
}

#[tauri::command]
pub async fn connection_state(
    state: State<'_, AppState>,
) -> Result<ConnectionState, CommandError> {
    Ok(state.supervisor.state())
}
