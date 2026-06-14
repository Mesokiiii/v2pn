//! System-tray integration: builds the icon, the right-click menu, and
//! wires up its actions. Closing the main window hides it; the app
//! lives in the tray with a single icon next to the clock. Same UX as
//! Telegram, Discord, qBittorrent — the muscle memory v2pn users
//! already have for "background app on Windows".
//!
//! Public API: [`install`] is the only entry point. Call it once during
//! Tauri `setup`, after `AppState` has been registered.

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager};

use crate::commands;

/// Mount the tray icon, build the menu, register both event handlers
/// (click on the icon, click on a menu item).
///
/// `app` here is the borrow we have during `setup`; we clone an
/// `AppHandle` out of it for the closures so they live for the
/// process lifetime.
pub fn install(app: &App) -> tauri::Result<()> {
    let handle = app.handle().clone();

    let show_item = MenuItemBuilder::with_id("show", "Открыть v2pn").build(&handle)?;
    let hide_item =
        MenuItemBuilder::with_id("hide", "Свернуть в трей").build(&handle)?;
    let disconnect_item =
        MenuItemBuilder::with_id("disconnect", "Отключить VPN").build(&handle)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Выход").build(&handle)?;

    let menu = MenuBuilder::new(&handle)
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
        // Don't auto-show the menu on left-click — left-click toggles
        // the window. That's what Windows-native tray apps do.
        .show_menu_on_left_click(false)
        .menu(&menu)
        .on_menu_event(handle_menu)
        .on_tray_icon_event(handle_icon_click)
        .build(&handle)?;
    Ok(())
}

/// Right-click menu dispatcher. Each branch ends with either a window
/// op or a spawned shutdown — never anything that could block the
/// tray-event thread.
fn handle_menu(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
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
            if app.try_state::<commands::AppState>().is_some() {
                let app_h = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_h.state::<commands::AppState>();
                    commands::shutdown_session(
                        &state,
                        &app_h,
                        commands::ShutdownOpts::USER_DISCONNECT,
                    )
                    .await;
                });
            }
        }
        "quit" => {
            // Spawn the cleanup, exit on its completion. RunEvent::Exit
            // also fires below and re-runs the cleanup defensively, so
            // a crashed cleanup task here does not orphan sing-box.
            if app.try_state::<commands::AppState>().is_some() {
                let app_h = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_h.state::<commands::AppState>();
                    commands::shutdown_session(
                        &state,
                        &app_h,
                        commands::ShutdownOpts::PROCESS_EXIT,
                    )
                    .await;
                    app_h.exit(0);
                });
            } else {
                app.exit(0);
            }
        }
        _ => {}
    }
}

/// Left-click on the icon → toggle window visibility. Right-click
/// triggers the menu independently (handled by Tauri's tray plugin).
fn handle_icon_click(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
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
}
