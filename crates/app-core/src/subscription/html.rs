//! Resolve subscriptions hidden behind a Remnawave/Marzban/sub-store-style
//! HTML "happ-aware" landing page.
//!
//! Strategy
//! ========
//! 1. Parse the HTML body and look for **deep-link buttons** like
//!    `happ://add/<URL>`, `v2raytun://import/<URL>`, `sing-box://import/<URL>`,
//!    `clash://install-config?url=<URL>` — these almost always carry the real
//!    raw subscription URL inside.
//! 2. Look for inline JSON / meta tags / data-attributes on the page that
//!    mention `subscription_url`, `links` etc. — some panels surface them.
//! 3. Heuristically generate URL candidates by appending well-known
//!    convention paths (`?type=clash`, `/raw`, `?clash=true`, `/sing-box`,
//!    `?happ=true`, etc.) — works for the majority of "normal" panels that
//!    just gate by User-Agent or query parameter.
//!
//! The caller of [`extract_subscription_url`] is expected to follow the
//! returned candidate with another fetch + format detect cycle.

use std::collections::BTreeSet;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::error::{CoreError, CoreResult};

const SCHEMES: &[&str] = &[
    "happ://", "v2raytun://", "streisand://", "sing-box://", "sn://",
    "clash://", "clashmeta://", "shadowrocket://", "stash://", "loon://",
    "nekobox://", "nekoray://", "hiddify://", "karing://",
];

/// Parsed candidates from a single HTML body.
#[derive(Debug, Default, Clone)]
pub struct HtmlCandidates {
    /// Real subscription URLs extracted from deep-links / meta tags.
    pub urls: Vec<String>,
    /// Suggested webapp URL the user can open in their browser.
    pub webapp_url: Option<String>,
}

/// Walk an HTML body and pull every plausible subscription URL out of it.
pub fn extract_subscription_url(body: &str, source_url: &str) -> HtmlCandidates {
    let mut urls: BTreeSet<String> = BTreeSet::new();

    // ---- 1. deep-link schemes ---------------------------------------------
    for scheme in SCHEMES {
        let mut from = 0;
        while let Some(idx) = body[from..].find(scheme) {
            let abs = from + idx;
            // collect from `scheme://` through the next quote / whitespace / >
            let tail = &body[abs..];
            let end = tail
                .find(|c: char| c == '"' || c == '\'' || c == '<' || c == '>' || c == '`' || c == ' ' || c == '\n' || c == '\r')
                .unwrap_or(tail.len());
            let link = &tail[..end];
            if let Some(u) = unwrap_deep_link(link) {
                urls.insert(u);
            }
            from = abs + scheme.len();
        }
    }

    // ---- 2. meta / data-* hints -------------------------------------------
    for needle in [
        "name=\"subscription-url\"",
        "name=\"sub-url\"",
        "data-subscription=\"",
        "data-sub-url=\"",
        "subscriptionUrl\":\"",
        "subscription_url\":\"",
        "\"subscriptionUrl\":",
    ] {
        if let Some(start) = body.find(needle) {
            // grab the next quoted value
            let after = &body[start..];
            if let Some(q1) = after.find(['"', '\'']) {
                let rest = &after[q1 + 1..];
                if let Some(q2) = rest.find(['"', '\'']) {
                    let v = &rest[..q2];
                    if v.starts_with("http") {
                        urls.insert(v.to_string());
                    }
                }
            }
        }
    }

    // ---- 3. plain http(s) URLs that look like subs ------------------------
    // Best-effort: any /sub/, /api/sub/, ?token=, .../subscription/UUID URLs.
    for cap in URL_RE.find_iter(body) {
        let u = cap.as_str();
        if looks_like_subscription_url(u) {
            urls.insert(u.to_string());
        }
    }

    // remove the source URL itself — it's the one we're trying to escape
    urls.remove(source_url);

    HtmlCandidates {
        urls: urls.into_iter().collect(),
        webapp_url: None,
    }
}

/// Decode a `scheme://verb/<payload>` deep-link into the real URL it carries.
///
/// We accept these payload encodings:
///   - URL-encoded: `happ://add/https%3A%2F%2Fexample.com%2Fsub%2FUUID`
///   - base64:      `happ://add/aHR0cHM6Ly9leGFtcGxlLmNvbS9zdWI...`
///   - raw:         `happ://add/https://example.com/sub/UUID`
fn unwrap_deep_link(link: &str) -> Option<String> {
    let after_scheme = link.splitn(2, "://").nth(1)?;
    // Skip a verb like "add/", "import/", "install-config?url=" if any.
    let payload = after_scheme
        .splitn(2, '/').nth(1)
        .or_else(|| after_scheme.splitn(2, '?').nth(1))
        .unwrap_or(after_scheme)
        .trim();

    if payload.is_empty() {
        return None;
    }

    // 1. URL-decoded?
    if payload.starts_with("http%3A") || payload.starts_with("https%3A") {
        if let Ok(decoded) = percent_decode_str(payload).decode_utf8() {
            return Some(decoded.into_owned());
        }
    }

    // 2. raw URL after the verb
    if payload.starts_with("http://") || payload.starts_with("https://") {
        return Some(payload.to_string());
    }

    // 3. inline `?url=...` (clash style)
    if let Some(after_eq) = payload.split_once("url=") {
        let value = after_eq.1.split('&').next().unwrap_or("");
        if let Ok(decoded) = percent_decode_str(value).decode_utf8() {
            if decoded.starts_with("http") {
                return Some(decoded.into_owned());
            }
        }
    }

    // 4. base64 of a URL?
    if let Ok(bytes) = crate::subscription::base64::decode_loose(payload) {
        if let Ok(s) = std::str::from_utf8(&bytes) {
            let s = s.trim();
            if s.starts_with("http://") || s.starts_with("https://") {
                return Some(s.to_string());
            }
        }
    }

    None
}

fn looks_like_subscription_url(u: &str) -> bool {
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return false;
    }
    let lc = u.to_ascii_lowercase();
    lc.contains("/sub/")
        || lc.contains("/api/sub/")
        || lc.contains("/subscription/")
        || lc.contains("/api/subscription/")
        || lc.contains("subconverter")
}

use once_cell::sync::Lazy;
use regex::Regex;
static URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"https?://[a-zA-Z0-9._/\-?&=:%+~@#]+"#).unwrap()
});

/// Generate a list of *convention-based* subscription URL candidates from a
/// given source URL. Used when the HTML body didn't surface anything obvious
/// — we just probe well-known formats panels respond to.
pub fn convention_candidates(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(u) = Url::parse(source) else {
        return out;
    };
    let base = format!("{}://{}", u.scheme(), u.host_str().unwrap_or(""));
    let path = u.path();

    // Query-style hints — these are the most common.
    for q in [
        "?clash=true", "?type=clash", "?clash-meta=true",
        "?type=v2ray", "?type=vless", "?type=raw",
        "?happ=true", "?type=happ", "?format=happ",
        "?type=sing-box", "?format=sing-box", "?singbox=true",
        "?type=streisand", "?type=shadowrocket", "?type=loon",
        "?type=stash", "?type=mihomo",
    ] {
        out.push(format!("{base}{path}{q}"));
    }

    // Suffix-style hints.
    for suffix in [
        "/raw", "/sub", "/links", "/v2ray", "/clash", "/clash-meta",
        "/sing-box", "/singbox", "/happ", "/streisand",
    ] {
        out.push(format!("{base}{path}{suffix}"));
    }

    out
}

/// Final-failure error returned to the UI when nothing else worked.
///
/// Carries enough info for the user to fall back to the web flow.
#[derive(Debug, Clone)]
pub struct HtmlLanderHint {
    pub source_url: String,
    pub webapp_url: Option<String>,
    pub probed_candidates: Vec<String>,
    pub message: String,
}

impl HtmlLanderHint {
    pub fn into_error(self) -> CoreError {
        CoreError::Parse(format!(
            "subscription panel returned an HTML landing page only.\n\
             We probed {} alternative endpoints — none worked.\n\
             {}",
            self.probed_candidates.len(),
            self.message
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_happ_add_with_raw_url() {
        let s = "happ://add/https://example.com/sub/abc";
        assert_eq!(
            unwrap_deep_link(s).as_deref(),
            Some("https://example.com/sub/abc")
        );
    }

    #[test]
    fn unwraps_clash_install_config_url() {
        let s = "clash://install-config?url=https%3A%2F%2Fexample.com%2Fsub%2Fabc&name=Foo";
        assert_eq!(
            unwrap_deep_link(s).as_deref(),
            Some("https://example.com/sub/abc")
        );
    }

    #[test]
    fn unwraps_sing_box_import_base64() {
        let url = "https://example.com/sub/abc";
        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            url.as_bytes(),
        );
        let link = format!("sing-box://import/{b64}");
        assert_eq!(unwrap_deep_link(&link).as_deref(), Some(url));
    }

    #[test]
    fn extracts_from_html_with_button() {
        let html = r#"
            <html><body>
                <a href="happ://add/https://api.example.com/sub/UUID-here">Connect</a>
            </body></html>
        "#;
        let r = extract_subscription_url(html, "https://other/page");
        assert_eq!(r.urls, vec!["https://api.example.com/sub/UUID-here".to_string()]);
    }

    #[test]
    fn convention_candidates_basic() {
        let v = convention_candidates("https://api.example.com/api/sub/UUID");
        assert!(v.iter().any(|s| s.contains("?clash=true")));
        assert!(v.iter().any(|s| s.contains("/raw")));
        assert!(v.iter().any(|s| s.contains("?type=happ")));
    }
}
