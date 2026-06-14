//! Subscription IPC commands: download + parse a remote subscription
//! URL, parse pasted body text, parse a single proxy URI.

use app_core::profile::ProxyProfile;
use app_core::subscription::fetch::{fetch_subscription, parse_body, FetchOptions};
use app_core::subscription::ParsedSubscription;

use super::CommandError;

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
