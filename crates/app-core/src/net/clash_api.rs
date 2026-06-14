//! Tiny typed wrapper around the sing-box clash API.
//!
//! Every caller used to build its own `reqwest::Client`, hand-roll
//! `bearer_auth(secret)` on each request, and string-format the URL —
//! that meant 4 places (switch_server, outbound_health, watchdog,
//! state_validator) silently disagreeing about timeout / no_proxy /
//! header conventions whenever someone touched one of them. This
//! module owns those decisions in one place and exposes a tight
//! method-per-endpoint surface.
//!
//! The clash API is the sing-box-internal control plane (set active
//! outbound, list connections, probe latency). It binds to
//! `127.0.0.1:<port>` and is gated by a Bearer token we generate per
//! connection and inject into the config — see
//! `Supervisor::rotate_clash_secret`.

use std::time::Duration;

use serde::Deserialize;

use crate::outbound_health::OutboundHealth;

/// HTTP timeout for short, latency-sensitive control calls. Matched
/// against the watchdog's expectation; tightening this would cause
/// false self-heal triggers under transient OS scheduler hiccups.
const SHORT_TIMEOUT: Duration = Duration::from_millis(1500);

/// Default URL the clash delay endpoint dials through the named
/// outbound. Tiny payload (204 No Content), globally CDN-served.
const PROBE_URL: &str = "https://www.gstatic.com/generate_204";

/// Per-probe upper bound. sing-box clash_api itself returns 504 around
/// the same window, so we keep them aligned.
const PROBE_TIMEOUT_MS: u32 = 4500;

/// Owns the connection details for one sing-box clash API endpoint.
/// Cheap to construct — the inner `reqwest::Client` is reused across
/// every method on a given [`Client`].
pub struct Client {
    port: u16,
    secret: Option<String>,
    http: reqwest::Client,
}

impl Client {
    /// New client bound to a localhost port. `secret` is the per-
    /// session Bearer token from `Supervisor::clash_secret()`; pass
    /// `None` only on the short-lived window between supervisor stop
    /// and shutdown (calls without the secret will be rejected with
    /// HTTP 401 anyway).
    pub fn new(port: u16, secret: Option<String>) -> reqwest::Result<Self> {
        let http = reqwest::Client::builder()
            // The system proxy is *us* — we'd loop forever if reqwest
            // tried to dial localhost through ourselves.
            .no_proxy()
            .timeout(SHORT_TIMEOUT)
            .build()?;
        Ok(Self { port, secret, http })
    }

    /// Convenience for callers that have a `&str` secret reference.
    pub fn from_ref(port: u16, secret: Option<&str>) -> reqwest::Result<Self> {
        Self::new(port, secret.map(|s| s.to_string()))
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    /// Stamp the Bearer header on a request builder. Pulled out so
    /// every method routes through the same authentication path —
    /// changing the auth scheme later is a one-line edit here.
    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.secret {
            Some(s) => req.bearer_auth(s),
            None => req,
        }
    }

    /// `GET /version` — the canonical "is sing-box's HTTP stack
    /// alive?" health check. Used by the watchdog and state
    /// validator. Returns the raw response so callers can branch on
    /// the precise status code (401/403 ⇒ secret out of sync,
    /// 404 ⇒ binary mismatch, 5xx ⇒ deadlock).
    pub async fn get_version(&self) -> reqwest::Result<reqwest::Response> {
        let req = self.http.get(self.url("/version"));
        self.auth(req).send().await
    }

    /// `GET /proxies/proxy` — return the currently selected outbound
    /// tag, or `None` on any error / network failure. Used by the
    /// outbound-health periodic loop to discover what tag to probe.
    pub async fn current_active_tag(&self) -> Option<String> {
        let req = self.http.get(self.url("/proxies/proxy"));
        let resp = self.auth(req).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = resp.text().await.ok()?;
        let parsed: ProxyState = serde_json::from_str(&body).ok()?;
        parsed.now
    }

    /// `PUT /proxies/proxy {"name": tag}` — hot-swap the active
    /// outbound on the `proxy` selector. Returns Ok on 2xx, Err with
    /// status + body on anything else.
    pub async fn switch_outbound(&self, tag: &str) -> Result<(), SwitchError> {
        let req = self
            .http
            .put(self.url("/proxies/proxy"))
            .json(&serde_json::json!({ "name": tag }));
        let resp = self
            .auth(req)
            .send()
            .await
            .map_err(|e| SwitchError {
                status: 0,
                body: format!("send: {e}"),
            })?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(SwitchError {
            status: status.as_u16(),
            body,
        })
    }

    /// `DELETE /connections` — forcibly close every active TCP/UDP
    /// session sing-box owns. Best-effort: failures are surfaced as
    /// the inner reqwest error so callers can log them at debug
    /// without changing control flow. Used after a hot-swap so
    /// long-lived connections don't keep streaming through the old
    /// outbound.
    pub async fn close_all_connections(&self) -> reqwest::Result<reqwest::Response> {
        let req = self.http.delete(self.url("/connections"));
        self.auth(req).send().await
    }

    /// `GET /proxies/{tag}/delay?…` — sing-box itself dials a
    /// synthetic HTTP request through the named outbound and reports
    /// the real round-trip in milliseconds. The most accurate way to
    /// know whether a *specific* server is reachable through the full
    /// REALITY+TLS+flow stack.
    pub async fn probe_outbound(&self, tag: &str) -> OutboundHealth {
        // We rebuild a fresh client for this call alone because the
        // probe deserves a longer timeout than our default — sing-box
        // returns 504 around 4.5 s, we give it 6 s of headroom.
        let probe_client = match reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(PROBE_TIMEOUT_MS as u64 + 1500))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return OutboundHealth::failure(tag, format!("client: {e}"));
            }
        };
        let path = format!(
            "/proxies/{}/delay?url={}&timeout={}",
            urlencoding(tag),
            urlencoding(PROBE_URL),
            PROBE_TIMEOUT_MS
        );
        let req = probe_client.get(self.url(&path));
        let req = match &self.secret {
            Some(s) => req.bearer_auth(s),
            None => req,
        };
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return OutboundHealth::failure(tag, format!("request: {e}")),
        };
        let status = resp.status();
        if !status.is_success() {
            let reason = match status.as_u16() {
                408 | 504 => "timeout".to_string(),
                code => format!("http {code}"),
            };
            return OutboundHealth::failure(tag, reason);
        }
        let body = resp.text().await.unwrap_or_default();
        match serde_json::from_str::<DelayBody>(&body) {
            Ok(DelayBody { delay: Some(ms) }) => OutboundHealth::success(tag, ms),
            Ok(_) => OutboundHealth::failure(tag, "malformed body (no delay field)"),
            Err(e) => OutboundHealth::failure(tag, format!("parse: {e}")),
        }
    }
}

/// Returned by [`Client::switch_outbound`] when sing-box rejects the
/// switch. UI surfaces `status` + `body` verbatim.
#[derive(Debug, Clone)]
pub struct SwitchError {
    pub status: u16,
    pub body: String,
}

impl std::fmt::Display for SwitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.body)
        } else {
            write!(f, "clash_api {} — {}", self.status, self.body)
        }
    }
}

impl std::error::Error for SwitchError {}

/* ----- response shapes --------------------------------------------------- */

#[derive(Debug, Deserialize)]
struct DelayBody {
    /// Mihomo / clash forks report `meanDelay` in some firmwares;
    /// sing-box uses `delay`. Our deployment target is sing-box, so
    /// we accept just the simple shape.
    delay: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProxyState {
    now: Option<String>,
}

/* ----- minimal URL-encoder ---------------------------------------------- */

/// Pulling `url` / `percent-encoding` for two characters would be
/// overkill; this hand-roll covers the only inputs we send (outbound
/// tags and the gstatic probe URL).
fn urlencoding(input: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
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
    async fn probe_against_dead_port_returns_failure() {
        let client = Client::new(1, None).expect("build");
        let h = client.probe_outbound("anything").await;
        assert!(h.latency_ms.is_none());
        assert!(h.error.is_some());
    }
}
