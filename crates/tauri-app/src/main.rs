// Prevent the extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bootstrap;
mod commands;
mod daemons;
mod power;
mod tray;

use tauri::{Manager, RunEvent, WindowEvent};
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
            // First-boot wiring lives in dedicated submodules now.
            // Each call below is a self-contained subsystem; the order
            // is load-bearing (recovery before supervisor build, daemons
            // and tray after AppState is registered) but every call is
            // pure side-effect on the same pp borrow, so adding /
            // removing one is local and safe.
            let bootstrapped = bootstrap::run(app)?;
            daemons::install_all(&app.handle().clone(), bootstrapped.supervisor.clone());
            power::install(&app.handle().clone());
            tray::install(app)?;
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
            commands::diagnostics::ping,
            commands::subscription::subscription_fetch,
            commands::subscription::subscription_parse_text,
            commands::subscription::subscription_parse_uri,
            commands::connection::connect,
            commands::connection::connect_subscription,
            commands::connection::switch_server,
            commands::connection::disconnect,
            commands::connection::connection_state,
            commands::connection::active_server_id,
            commands::routing::set_connection_mode,
            commands::routing::set_routing,
            commands::routing::get_connection_options,
            commands::routing::probe_latency_batch,
            commands::elevation::elevation_status,
            commands::elevation::restart_as_admin,
            commands::diagnostics::open_logs_folder,
            commands::diagnostics::diagnostics,
            commands::diagnostics::repair_network,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Drive the event loop with our own callback so we can intercept Exit.
    app.run(|handle, event| {
        if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
            tracing::info!("RunEvent: shutdown — performing final cleanup");
            // We block on a fresh Tokio runtime here because Tauri's
            // own async_runtime is being torn down.
            if handle.try_state::<commands::AppState>().is_some() {
                let handle_clone = handle.clone();
                if let Ok(rt) = tokio::runtime::Runtime::new() {
                    rt.block_on(async move {
                        let state = handle_clone.state::<commands::AppState>();
                        commands::shutdown_session(
                            &state,
                            &handle_clone,
                            commands::ShutdownOpts::PROCESS_EXIT,
                        )
                        .await;
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
