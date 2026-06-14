//! Outbound health snapshot type, plus thin wrappers that delegate the
//! actual HTTP work to [`crate::clash_api::Client`]. Kept as a tiny
//! module of its own because the snapshot type is part of the
//! Tauri/IPC surface (sent to the frontend over the
//! `outbound-health` event) and we don't want UI changes to drag in
//! the full clash-API client API.

use serde::{Deserialize, Serialize};

/// What we report to the UI for one probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundHealth {
    /// Outbound tag we probed (e.g. `srv-8fbd0fb6960a`). Matches
    /// whatever `server_tag_for(profile_id)` returns.
    pub tag: String,
    /// Round-trip in milliseconds, or `None` if the probe failed /
    /// timed out.
    pub latency_ms: Option<u32>,
    /// Free-form reason on failure (`"timeout"`, `"http 502"`, …) —
    /// UI surfaces this in tooltips / error toasts.
    pub error: Option<String>,
    /// Wall-clock unix seconds when the probe completed.
    pub at: i64,
}

impl OutboundHealth {
    /// Construct a successful result for `tag` with the given
    /// round-trip in milliseconds.
    pub fn success(tag: &str, latency_ms: u32) -> Self {
        Self {
            tag: tag.to_string(),
            latency_ms: Some(latency_ms),
            error: None,
            at: now_unix(),
        }
    }

    /// Construct a failure result with a free-form reason.
    pub fn failure(tag: &str, error: impl Into<String>) -> Self {
        Self {
            tag: tag.to_string(),
            latency_ms: None,
            error: Some(error.into()),
            at: now_unix(),
        }
    }
}

/// Probe a single outbound by tag. Convenience wrapper around
/// [`crate::clash_api::Client::probe_outbound`] for callers that only
/// want a one-shot probe and don't need to keep a `Client` around.
pub async fn probe(clash_api_port: u16, tag: &str, secret: Option<&str>) -> OutboundHealth {
    let client = match crate::clash_api::Client::from_ref(clash_api_port, secret) {
        Ok(c) => c,
        Err(e) => return OutboundHealth::failure(tag, format!("client: {e}")),
    };
    client.probe_outbound(tag).await
}

/// Look up which outbound tag is currently active on the `proxy`
/// selector. Convenience wrapper around
/// [`crate::clash_api::Client::current_active_tag`].
pub async fn current_active_tag(
    clash_api_port: u16,
    secret: Option<&str>,
) -> Option<String> {
    let client = crate::clash_api::Client::from_ref(clash_api_port, secret).ok()?;
    client.current_active_tag().await
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probe_fails_on_dead_port_without_panic() {
        let h = probe(1, "anything", None).await;
        assert!(h.latency_ms.is_none());
        assert!(h.error.is_some());
    }
}
