//! Active-outbound health probing via the clash API.
//!
//! Wraps the `GET /proxies/{tag}/delay` endpoint that sing-box exposes when
//! the clash API is enabled. Sings-box itself sends a synthetic HTTP request
//! through the named outbound and reports the round-trip in milliseconds —
//! that's the most accurate way to know whether a *specific* server is
//! reachable, since it exercises the full proxy chain (REALITY, TLS, flow
//! control, etc.) rather than just liveness of the local clash listener.
//!
//! Used by:
//!  * `switch_server` — fires a one-shot probe right after a hot-switch so
//!    the UI can flag dead servers immediately.
//!  * The background `outbound_health_loop` — periodic probe that emits
//!    health events while the supervisor is connected.
//!
//! All requests go via `reqwest::Client::no_proxy()`; otherwise the
//! enclosing Windows system proxy (which we just set!) would steal them.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// What we report to the UI for one probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundHealth {
    /// Outbound tag we probed (e.g. `srv-8fbd0fb6960a`). Equal to whatever
    /// `server_tag_for(profile_id)` returns.
    pub tag: String,
    /// Round-trip in milliseconds, or `None` if the probe failed / timed out.
    pub latency_ms: Option<u32>,
    /// Free-form reason on failure (`"timeout"`, `"http 502"`, …) — UI
    /// surfaces this in tooltips / error toasts.
    pub error: Option<String>,
    /// Wall-clock unix seconds when the probe completed.
    pub at: i64,
}

/// Default URL the clash API uses to perform the synthetic probe. Picked
/// for its tiny payload (204 No Content) and global CDN reachability.
const PROBE_URL: &str = "https://www.gstatic.com/generate_204";

/// Per-probe upper bound. sing-box returns a 504 from clash_api after
/// roughly the same window, so we keep them aligned.
const PROBE_TIMEOUT_MS: u32 = 4500;

/// Probe a single outbound by tag and return a [`OutboundHealth`].
///
/// Never panics, never throws — failures are reported via
/// `OutboundHealth { latency_ms: None, error: Some(_) }`.
///
/// `secret` is the clash_api Bearer token. Pass `None` only on the
/// short-lived window between supervisor stop and shutdown (when there
/// is no live clash_api anyway). Calls without the secret will be
/// rejected with HTTP 401, which we surface as `error: "http 401"`.
pub async fn probe(clash_api_port: u16, tag: &str, secret: Option<&str>) -> OutboundHealth {
    let url = format!(
        "http://127.0.0.1:{}/proxies/{}/delay?url={}&timeout={}",
        clash_api_port,
        urlencoding(tag),
        urlencoding(PROBE_URL),
        PROBE_TIMEOUT_MS
    );

    let client = match reqwest::Client::builder()
        .no_proxy()
        // Add a small grace period over PROBE_TIMEOUT_MS so the clash API
        // has a chance to return a 504 instead of us aborting locally.
        .timeout(Duration::from_millis(PROBE_TIMEOUT_MS as u64 + 1500))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return OutboundHealth {
                tag: tag.to_string(),
                latency_ms: None,
                error: Some(format!("client: {e}")),
                at: now_unix(),
            };
        }
    };

    let mut req = client.get(&url);
    if let Some(s) = secret {
        req = req.bearer_auth(s);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return OutboundHealth {
                tag: tag.to_string(),
                latency_ms: None,
                error: Some(format!("request: {e}")),
                at: now_unix(),
            };
        }
    };

    let status = resp.status();
    if !status.is_success() {
        // 408 / 504 / 502 from clash_api means "the upstream was unreachable
        // within the timeout". Surface that distinct from "client error".
        return OutboundHealth {
            tag: tag.to_string(),
            latency_ms: None,
            error: Some(match status.as_u16() {
                408 | 504 => "timeout".to_string(),
                code => format!("http {code}"),
            }),
            at: now_unix(),
        };
    }

    // Body shape: {"delay":<u32>} for sing-box / clash; missing field on a
    // few mihomo-flavoured forks is treated as a soft failure.
    let body = resp.text().await.unwrap_or_default();
    let parsed: Result<DelayBody, _> = serde_json::from_str(&body);
    match parsed {
        Ok(DelayBody { delay: Some(ms) }) => OutboundHealth {
            tag: tag.to_string(),
            latency_ms: Some(ms),
            error: None,
            at: now_unix(),
        },
        Ok(_) => OutboundHealth {
            tag: tag.to_string(),
            latency_ms: None,
            error: Some("malformed body (no delay field)".to_string()),
            at: now_unix(),
        },
        Err(e) => OutboundHealth {
            tag: tag.to_string(),
            latency_ms: None,
            error: Some(format!("parse: {e}")),
            at: now_unix(),
        },
    }
}

/// Ask the clash API which outbound is currently selected on the `proxy`
/// selector. Returns `None` on any failure.
pub async fn current_active_tag(clash_api_port: u16, secret: Option<&str>) -> Option<String> {
    let url = format!("http://127.0.0.1:{clash_api_port}/proxies/proxy");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(1500))
        .build()
        .ok()?;
    let mut req = client.get(&url);
    if let Some(s) = secret {
        req = req.bearer_auth(s);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.text().await.ok()?;
    let parsed: ProxyState = serde_json::from_str(&body).ok()?;
    parsed.now
}

#[derive(Debug, Deserialize)]
struct DelayBody {
    /// Mihomo / clash report `meanDelay` in some firmwares; sing-box uses
    /// `delay`. We accept either by serde rename — but sing-box is our
    /// target, keep the simple shape.
    delay: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProxyState {
    now: Option<String>,
}

/// Minimal URL-encoder — we only need to escape characters that show up in
/// outbound tags or the probe URL. Pulling in `url` or `percent-encoding`
/// for two characters would be overkill.
fn urlencoding(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            other => {
                use std::fmt::Write;
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
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

    #[test]
    fn urlencoding_keeps_unreserved() {
        assert_eq!(urlencoding("srv-1234567890ab"), "srv-1234567890ab");
    }

    #[test]
    fn urlencoding_escapes_special() {
        assert_eq!(urlencoding("a/b?c"), "a%2Fb%3Fc");
        assert_eq!(urlencoding(":"), "%3A");
    }

    #[tokio::test]
    async fn probe_fails_on_dead_port_without_panic() {
        // Port 1 is unused on every sane host. The probe must surface a
        // structured failure, not panic or block forever.
        let h = probe(1, "anything", None).await;
        assert!(h.latency_ms.is_none());
        assert!(h.error.is_some());
    }
}
