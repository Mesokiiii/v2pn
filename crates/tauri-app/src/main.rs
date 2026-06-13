// Prevent the extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::sync::Arc;

use app_core::power::{register as register_power, PowerEvent};
use app_core::supervisor::{resolve_singbox_binary, Supervisor};
use app_core::watchdog;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() {
    // Logging: console (stderr) for `tauri dev`, plus a daily-rotated file
    // under `<app_data_dir>/logs/v2pn.log` so even silent admin-mode
    // launches leave a paper trail. Default verbosity: TRACE for our crates,
    // INFO everywhere else. Override at runtime with RUST_LOG.
    //
    // The base path here MUST match what Tauri exposes via
    // `app.path().app_data_dir()` so `open_logs_folder` lands in the same
    // place. Tauri builds that path from the bundle identifier in
    // tauri.conf.json (`io.v2pn.app`), joined onto `dirs::data_dir()`
    // (= `%APPDATA%` on Windows / `~/Library/Application Support` on macOS /
    // `~/.local/share` on Linux).
    const TAURI_IDENTIFIER: &str = "io.v2pn.app";
    let log_dir = directories::BaseDirs::new()
        .map(|b| b.data_dir().join(TAURI_IDENTIFIER).join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "v2pn.log");
    let (file_writer, _file_guard) = tracing_appender::non_blocking(file_appender);
    // Leak the guard intentionally: we want it for the whole process lifetime.
    Box::leak(Box::new(_file_guard));

    let default_filter = if cfg!(debug_assertions) {
        "trace,hyper=info,tower=info,tao=info,wry=info,tauri=info,rustls=info,h2=info,reqwest=info,serde_json=info"
    } else {
        "info,v2pn=debug,app_core=debug"
    };

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)))
        .with(fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_line_number(true))
        .with(fmt::layer()
            .with_writer(file_writer)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_line_number(true))
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        log_dir = %log_dir.display(),
        "v2pn booting"
    );

    install_panic_hook();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            tracing::info!(?args, ?cwd, "second instance launched, focusing main window");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let exe_dir = std::env::current_exe()?
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let binary = resolve_singbox_binary(&exe_dir).ok_or_else(|| {
                anyhow::anyhow!(
                    "sing-box binary not found near {}. Run scripts/fetch-singbox.ps1 first.",
                    exe_dir.display()
                )
            })?;

            let runtime_dir = app.path().app_data_dir()?.join("runtime");
            std::fs::create_dir_all(&runtime_dir)?;

            // 1. Bootstrap recovery — clean up after a previous crashed run
            //    BEFORE we let the user start a new connection.
            let _outcome = commands::run_startup_recovery(&runtime_dir);

            // 2. Build supervisor + state holder.
            let supervisor = Arc::new(Supervisor::new(binary, runtime_dir.clone())?);
            commands::spawn_event_bridges(app.handle().clone(), supervisor.clone());
            app.manage(commands::AppState::new(supervisor.clone(), runtime_dir));

            // 3. Spawn the watchdog: ping clash_api every 2s; on 3 misses
            //    request a self-heal (kill + restart from last config).
            //    Runs on the Tauri async runtime.
            let opts = tauri::async_runtime::block_on(async {
                app.state::<commands::AppState>().options.lock().await.clone()
            });
            let stop = watchdog::new_stop_handle();
            // Keep the stop-handle alive for the process lifetime so the
            // task isn't aborted when this scope exits.
            static WATCHDOG_STOP: std::sync::OnceLock<watchdog::StopHandle> =
                std::sync::OnceLock::new();
            let _ = WATCHDOG_STOP.set(stop.clone());
            tauri::async_runtime::spawn(watchdog::run(
                supervisor.clone(),
                opts.clash_api_port,
                stop,
            ));

            // 3b. Spawn the state validator: every 10s while Connected,
            //     triple-checks (process alive | clash_api responds |
            //     mixed-port listening). Two consecutive bad ticks → ask
            //     the supervisor to self-heal. Independent from watchdog
            //     and outbound-health: each catches a different failure
            //     mode, all three can heal in parallel without thrashing
            //     because the auto-restart loop in supervisor is guarded
            //     by `auto_restart_in_flight`.
            let validator_stop = app_core::state_validator::new_stop_handle();
            static VALIDATOR_STOP: std::sync::OnceLock<app_core::state_validator::StopHandle> =
                std::sync::OnceLock::new();
            let _ = VALIDATOR_STOP.set(validator_stop.clone());
            tauri::async_runtime::spawn(app_core::state_validator::run(
                supervisor.clone(),
                opts.mixed_port,
                opts.clash_api_port,
                validator_stop,
            ));

            // 4. Register OS power-suspend handler.
            //
            //    Suspend  → if the user was Connected, stash the LastConnectIntent
            //               and run the same emergency-disconnect sequence as
            //               before (kill sing-box, restore OS proxy via Drop).
            //               Without the disconnect, the laptop wakes up with
            //               sing-box dead, the system proxy still pointing at
            //               127.0.0.1:7890, and every Edge/Outlook/Steam tab
            //               in connection-refused hell. Better to surface the
            //               outage cleanly and reconnect immediately on resume.
            //
            //    Resume   → if `suspend_was_connected` is set, replay the
            //               stored `LastConnectIntent` through the normal
            //               connect path on a 3 s delay (gives Wi-Fi /
            //               Ethernet / VPN-aware NIC drivers time to come
            //               back). The user sees: brief offline → green
            //               connection back, no manual click required.
            let app_handle = app.handle().clone();
            register_power(move |event| match event {
                PowerEvent::Suspend => {
                    tracing::warn!(target: "power", "system suspending — emergency disconnect");
                    let app_handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let (supervisor, guard_slot, was_connected_slot) = {
                            let state = app_handle.state::<commands::AppState>();
                            (state.supervisor.clone(), state.guard.clone(), state.suspend_was_connected.clone())
                        };
                        // Capture the connection state *before* we pull the
                        // plug — the supervisor flips to Stopping/Idle as
                        // soon as we call stop().
                        let was_connected = matches!(
                            supervisor.state(),
                            app_core::supervisor::ConnectionState::Connected
                                | app_core::supervisor::ConnectionState::Starting
                        );
                        *was_connected_slot.lock().await = was_connected;

                        let _ = supervisor.stop().await;
                        let taken = { guard_slot.lock().await.take() };
                        if let Some(g) = taken {
                            let _ = g.release();
                        }
                    });
                }
                PowerEvent::Resume => {
                    tracing::info!(target: "power", "system resumed");
                    let app_handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let (was_connected, last_intent) = {
                            let state = app_handle.state::<commands::AppState>();
                            let was = *state.suspend_was_connected.lock().await;
                            // Consume the flag so a spurious second
                            // RESUME notification (Windows sometimes
                            // double-fires across S3↔modern-standby
                            // boundaries) doesn't trigger a duplicate
                            // reconnect.
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

                        // Hold for the network stack to settle. 3 s is the
                        // sweet spot we measured on the test laptop: less
                        // than 1 s and reqwest tends to grab a stale
                        // route, more than 5 s and the user notices the
                        // VPN-down window.
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        tracing::info!(target: "power",
                            profiles = intent.profiles.len(),
                            selected = %intent.selected_id,
                            mode = ?intent.mode,
                            "auto-reconnecting after resume");

                        // Replay through the public command — same path as
                        // the user clicking Connect from the UI, including
                        // sanitiser, guard, supervisor.start, post-switch
                        // probe. Failures bubble into the v2pn.log; the UI
                        // shows whatever connection-state event lands.
                        let app_state = app_handle.state::<commands::AppState>();
                        let res = commands::connect_subscription_internal(
                            intent.profiles,
                            intent.selected_id,
                            Some(intent.mode),
                            app_state,
                            app_handle.clone(),
                        ).await;
                        if let Err(e) = res {
                            tracing::error!(target: "power",
                                "auto-reconnect failed: {}", e.message);
                        }
                    });
                }
            });

            // 5. Auto-cleanup on Failed: when the supervisor reports Failed
            //    (sing-box died) we release the proxy guard so the user can
            //    immediately try a different server without seeing
            //    "already connected".
            {
                let app_handle = app.handle().clone();
                let mut state_rx = supervisor.subscribe_state();
                tauri::async_runtime::spawn(async move {
                    use app_core::supervisor::ConnectionState;
                    while let Ok(s) = state_rx.recv().await {
                        if matches!(s, ConnectionState::Failed { .. }) {
                            let guard_slot = {
                                let state = app_handle.state::<commands::AppState>();
                                state.guard.clone()
                            };
                            let taken = { guard_slot.lock().await.take() };
                            if let Some(g) = taken {
                                tracing::warn!(target: "auto-cleanup",
                                    "engine reported Failed — releasing proxy guard");
                                let _ = g.release();
                            }
                        }
                    }
                });
            }

            // 6. Background outbound health probe: while Connected, every
            //    `HEALTH_INTERVAL`, ask sing-box's clash API to dial the
            //    currently selected outbound through the public probe URL.
            //    The result is broadcast via the `outbound-health` event so
            //    the UI can light up a 🟢/🟡/🔴 indicator and surface dead
            //    servers without waiting for the user to notice timeouts.
            //
            //    This is *separate* from `watchdog` — that one polls the
            //    clash API itself (sing-box liveness). This loop polls the
            //    upstream tunnel (server liveness). Different failure modes,
            //    different responses.
            {
                let app_handle = app.handle().clone();
                let supervisor = supervisor.clone();
                tauri::async_runtime::spawn(async move {
                    use app_core::supervisor::ConnectionState;
                    use std::time::Duration;
                    const HEALTH_INTERVAL: Duration = Duration::from_secs(20);
                    // Small grace period after Connected before the first
                    // probe so the freshly-spawned proxy gets a chance to
                    // settle (TLS handshake, REALITY exchange, etc.).
                    const FIRST_PROBE_DELAY: Duration = Duration::from_secs(3);

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
                            let state = app_handle.state::<commands::AppState>();
                            let p = state.options.lock().await.clash_api_port;
                            p
                        };
                        let secret = supervisor.clash_secret();

                        // Resolve the active tag from clash API rather than
                        // app state — keeps us in sync with hot-switches that
                        // might have been initiated by a different code path.
                        let Some(tag) =
                            app_core::outbound_health::current_active_tag(port, secret.as_deref()).await
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
                        let _ = app_handle.emit("outbound-health", &h);
                    }
                });
            }

            // 7. System tray.
            //
            //    Closing the main window hides it; the app keeps running
            //    in the background with a tray icon. Left-click on the
            //    tray icon → toggle the window. Right-click → menu with
            //    Show / Hide / Disconnect / Quit.
            //
            //    Quit is the *only* path that fully tears down the
            //    supervisor; everything else just hides UI. That mirrors
            //    every long-running Windows-tray app the user has muscle
            //    memory for (Telegram, Discord, qBittorrent, …).
            {
                let app_handle = app.handle().clone();
                let show_item = MenuItemBuilder::with_id("show", "Открыть v2pn").build(&app_handle)?;
                let hide_item = MenuItemBuilder::with_id("hide", "Свернуть в трей").build(&app_handle)?;
                let disconnect_item =
                    MenuItemBuilder::with_id("disconnect", "Отключить VPN").build(&app_handle)?;
                let quit_item =
                    MenuItemBuilder::with_id("quit", "Выход").build(&app_handle)?;
                let menu = MenuBuilder::new(&app_handle)
                    .item(&show_item)
                    .item(&hide_item)
                    .separator()
                    .item(&disconnect_item)
                    .separator()
                    .item(&quit_item)
                    .build()?;

                let _tray = TrayIconBuilder::with_id("main")
                    .tooltip("v2pn")
                    .icon(app.default_window_icon().unwrap().clone())
                    // Don't auto-show menu on left click; we want left
                    // click to toggle the window (Windows-native feel).
                    .show_menu_on_left_click(false)
                    .menu(&menu)
                    .on_menu_event(move |app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "hide" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.hide();
                            }
                        }
                        "disconnect" => {
                            if let Some(state) = app.try_state::<commands::AppState>() {
                                let supervisor = state.supervisor.clone();
                                let guard_slot = state.guard.clone();
                                let active = state.active_selected.clone();
                                let last_intent = state.last_intent.clone();
                                let app_h = app.clone();
                                tauri::async_runtime::spawn(async move {
                                    let _ = supervisor.stop().await;
                                    let taken = { guard_slot.lock().await.take() };
                                    if let Some(g) = taken {
                                        let _ = g.release();
                                    }
                                    *active.lock().await = None;
                                    *last_intent.lock().await = None;
                                    let _ = app_h.emit("connection-state",
                                        supervisor.state());
                                });
                            }
                        }
                        "quit" => {
                            // Spawn the cleanup, but exit on its
                            // completion — RunEvent::Exit will *also*
                            // fire and re-run the cleanup path defensively.
                            if let Some(state) = app.try_state::<commands::AppState>() {
                                let supervisor = state.supervisor.clone();
                                let guard_slot = state.guard.clone();
                                let app_h = app.clone();
                                tauri::async_runtime::spawn(async move {
                                    let _ = supervisor.stop().await;
                                    let taken = { guard_slot.lock().await.take() };
                                    if let Some(g) = taken {
                                        let _ = g.release();
                                    }
                                    app_h.exit(0);
                                });
                            } else {
                                app.exit(0);
                            }
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            // Toggle main window visibility on left-click.
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                if w.is_visible().unwrap_or(false) {
                                    let _ = w.hide();
                                } else {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                    })
                    .build(&app_handle)?;
            }

            tracing::info!("v2pn started");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Don't actually close — fold into the tray instead. This
                // is the same behaviour as Telegram / Discord / qBittorrent
                // / every other "live in the background" app on Windows:
                // the user closing the main window is rarely a request to
                // disconnect the VPN, just to free screen real estate.
                //
                // Quitting for real is done via the tray menu's "Quit"
                // entry, which calls `app.exit(0)` after stopping the
                // engine; the RunEvent::Exit handler below still runs and
                // restores the OS proxy / kills sing-box on that path.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::subscription_fetch,
            commands::subscription_parse_text,
            commands::subscription_parse_uri,
            commands::connect,
            commands::connect_subscription,
            commands::switch_server,
            commands::disconnect,
            commands::connection_state,
            commands::active_server_id,
            commands::set_connection_mode,
            commands::get_connection_options,
            commands::probe_latency_batch,
            commands::elevation_status,
            commands::restart_as_admin,
            commands::open_logs_folder,
            commands::diagnostics,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Drive the event loop with our own callback so we can intercept Exit.
    app.run(|handle, event| {
        if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
            tracing::info!("RunEvent: shutdown — performing final cleanup");
            // 1. Stop sing-box
            // 2. Release guard (restores OS proxy + deletes state file)
            //
            // We block on a fresh tokio runtime here because Tauri's
            // async_runtime is being torn down.
            if let Some(state) = handle.try_state::<commands::AppState>() {
                let supervisor = state.supervisor.clone();
                let guard_slot = state.guard.clone();
                let rt = tokio::runtime::Runtime::new().ok();
                if let Some(rt) = rt {
                    rt.block_on(async move {
                        let _ = supervisor.stop().await;
                        if let Some(g) = guard_slot.lock().await.take() {
                            let _ = g.release();
                        }
                    });
                }
            }
        }
    });
}

/// Install a panic hook that logs the panic and lets the process exit. The
/// `ConnectionGuard`'s `Drop` impl runs as the stack unwinds, restoring the
/// OS proxy automatically.
///
/// We deliberately do NOT abort here — abort skips destructors, which would
/// break the very crash safety we're trying to provide.
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "panic",
            "unhandled panic: {}\nbacktrace:\n{:?}",
            info,
            std::backtrace::Backtrace::force_capture()
        );
        prev(info);
    }));
}
