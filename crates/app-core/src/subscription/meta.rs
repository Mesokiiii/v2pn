//! Subscription metadata extracted from HTTP headers.
//!
//! Spec (de-facto, originating from Shadowsocks-Android / Clash):
//!
//! ```text
//! subscription-userinfo: upload=12345; download=678910; total=53687091200; expire=1781308800
//! profile-update-interval: 12
//! profile-title: BUZZ VPN
//! profile-web-page-url: https://...
//! support-url: https://t.me/...
//! ```
//!
//! `expire` is a Unix timestamp in seconds; `total/upload/download` are bytes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionMeta {
    pub title: Option<String>,
    pub upload_bytes: Option<u64>,
    pub download_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    /// Unix timestamp (seconds) when the subscription expires.
    pub expire_at: Option<i64>,
    /// Recommended auto-update interval in hours.
    pub update_interval_hours: Option<u32>,
    pub web_page_url: Option<String>,
    pub support_url: Option<String>,
}

impl SubscriptionMeta {
    pub fn from_headers<H: HeaderLike>(headers: &H) -> Self {
        let mut meta = SubscriptionMeta::default();

        if let Some(v) = headers.get_first("profile-title") {
            // RFC 5987-ish: `profile-title: base64:<b64-utf8>`
            meta.title = Some(decode_profile_title(v.trim()));
        }
        if let Some(v) = headers.get_first("profile-update-interval") {
            meta.update_interval_hours = v.trim().parse().ok();
        }
        if let Some(v) = headers.get_first("profile-web-page-url") {
            meta.web_page_url = Some(v.trim().to_string());
        }
        if let Some(v) = headers.get_first("support-url") {
            meta.support_url = Some(v.trim().to_string());
        }
        if let Some(v) = headers.get_first("subscription-userinfo") {
            parse_userinfo(v, &mut meta);
        }

        meta
    }

    pub fn used_bytes(&self) -> Option<u64> {
        match (self.upload_bytes, self.download_bytes) {
            (Some(u), Some(d)) => Some(u.saturating_add(d)),
            (Some(u), None) => Some(u),
            (None, Some(d)) => Some(d),
            (None, None) => None,
        }
    }
}

fn parse_userinfo(value: &str, meta: &mut SubscriptionMeta) {
    for part in value.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "upload" => meta.upload_bytes = v.parse().ok(),
                "download" => meta.download_bytes = v.parse().ok(),
                "total" => meta.total_bytes = v.parse().ok(),
                "expire" => meta.expire_at = v.parse().ok(),
                _ => {}
            }
        }
    }
}

fn decode_profile_title(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("base64:") {
        if let Ok(decoded) = crate::subscription::base64::decode_loose(rest) {
            if let Ok(s) = std::str::from_utf8(&decoded) {
                return s.to_string();
            }
        }
    }
    raw.to_string()
}

/// Trait so we can extract metadata from either real HTTP headers or a
/// user-supplied `HashMap` in tests.
pub trait HeaderLike {
    fn get_first(&self, name: &str) -> Option<&str>;
}

impl HeaderLike for std::collections::HashMap<String, String> {
    fn get_first(&self, name: &str) -> Option<&str> {
        self.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl HeaderLike for reqwest::header::HeaderMap {
    fn get_first(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(|v| v.to_str().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parses_userinfo_header() {
        let mut h = HashMap::new();
        h.insert(
            "subscription-userinfo".to_string(),
            "upload=10; download=20; total=100; expire=1781308800".to_string(),
        );
        h.insert("profile-title".to_string(), "BUZZ VPN".to_string());
        h.insert("profile-update-interval".to_string(), "12".to_string());

        let m = SubscriptionMeta::from_headers(&h);
        assert_eq!(m.upload_bytes, Some(10));
        assert_eq!(m.download_bytes, Some(20));
        assert_eq!(m.total_bytes, Some(100));
        assert_eq!(m.expire_at, Some(1781308800));
        assert_eq!(m.update_interval_hours, Some(12));
        assert_eq!(m.title.as_deref(), Some("BUZZ VPN"));
        assert_eq!(m.used_bytes(), Some(30));
    }

    #[test]
    fn decodes_base64_title() {
        let raw = format!(
            "base64:{}",
            base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                "🚀 BUZZ VPN".as_bytes()
            )
        );
        let mut h = HashMap::new();
        h.insert("profile-title".to_string(), raw);

        let m = SubscriptionMeta::from_headers(&h);
        assert_eq!(m.title.as_deref(), Some("🚀 BUZZ VPN"));
    }
}
