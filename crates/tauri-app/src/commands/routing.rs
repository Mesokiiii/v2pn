//! Connection-options getter/setter commands + on-demand latency probe.

use app_core::profile::ProxyProfile;
use app_core::singbox::config::{ConnectionMode, ConnectionOptions};
use tauri::State;

use super::{AppState, CommandError};

#[tauri::command]
pub async fn set_connection_mode(
    mode: ConnectionMode,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.options.lock().await.mode = mode;
    Ok(())
}

/// Replace the routing-bypass configuration. Empty `country_codes` and
/// empty `custom_rules` together mean "everything through the VPN" —
/// the pure-tunnel mode. Takes effect on the next `connect`; we don't
/// hot-reload the config because rule-set downloads happen at sing-box
/// boot and re-doing them would race with active connections.
#[tauri::command]
pub async fn set_routing(
    country_codes: Vec<String>,
    custom_rules: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let mut o = state.options.lock().await;
    o.bypass_country_codes = country_codes
        .into_iter()
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    o.custom_bypass_rules = custom_rules
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(())
}

#[tauri::command]
pub async fn get_connection_options(
    state: State<'_, AppState>,
) -> Result<ConnectionOptions, CommandError> {
    Ok(state.options.lock().await.clone())
}

/// One-shot latency probe over a batch of profiles. Used by the UI to
/// rank servers in the sidebar before the user connects. TCP-only — the
/// real REALITY-stack probe goes through `outbound_health` once a
/// session is up.
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
