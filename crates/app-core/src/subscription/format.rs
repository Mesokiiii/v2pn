//! Sniff the actual subscription format from a downloaded body.

use crate::subscription::base64::decode_loose;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionFormat {
    /// One or more `vless://` / `vmess://` / `trojan://` / `ss://` / `hy2://` / `tuic://`
    /// links separated by newlines. The most common format.
    UriList,
    /// The above, but base64-encoded as a single blob.
    Base64UriList,
    /// A full sing-box JSON config with `outbounds`.
    SingBoxJson,
    /// Top-level JSON array of Xray configs (Happ format).
    XrayArray,
    /// A Clash/Clash.Meta YAML config with `proxies:`.
    ClashYaml,
    /// HTML landing page (Remnawave universal page, etc.) — needs special handling.
    Html,
    /// Cannot determine.
    Unknown,
}

pub fn detect(body: &[u8]) -> SubscriptionFormat {
    let trimmed = trim_bom(body);
    // Take a generous head so subscription formats that lead with a big
    // JSON sub-tree (e.g. the v2rayN/Happ array, where the first entry's
    // `inbounds` runs ~3 KiB before the `outbounds` key shows up) are
    // still classified correctly.
    let head: String = trimmed
        .iter()
        .take(8192)
        .map(|&b| if b.is_ascii() { b as char } else { ' ' })
        .collect();
    let head_lc = head.to_ascii_lowercase();
    let head_t = head.trim_start();

    if head_lc.contains("<!doctype html") || head_lc.starts_with("<html") || head_lc.contains("<html") {
        return SubscriptionFormat::Html;
    }

    if head_t.starts_with('{') {
        if head.contains("\"outbounds\"") || head.contains("\"inbounds\"") {
            return SubscriptionFormat::SingBoxJson;
        }
    }

    if head_t.starts_with('[') {
        if crate::subscription::xray_array::looks_like(trimmed) {
            return SubscriptionFormat::XrayArray;
        }
    }

    if head_lc.contains("proxies:") || head_lc.contains("proxy-groups:") {
        return SubscriptionFormat::ClashYaml;
    }

    // URI list, plain
    if has_proxy_uri(&head_lc) {
        return SubscriptionFormat::UriList;
    }

    // Try base64 path
    if looks_like_base64(&head) {
        if let Ok(decoded) = decode_loose(std::str::from_utf8(trimmed).unwrap_or("")) {
            let inner = String::from_utf8_lossy(&decoded[..decoded.len().min(2048)]).to_ascii_lowercase();
            if has_proxy_uri(&inner) {
                return SubscriptionFormat::Base64UriList;
            }
        }
    }

    SubscriptionFormat::Unknown
}

fn trim_bom(body: &[u8]) -> &[u8] {
    if body.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &body[3..]
    } else {
        body
    }
}

fn has_proxy_uri(s: &str) -> bool {
    const SCHEMES: &[&str] = &[
        "vless://", "vmess://", "trojan://", "ss://", "ssr://",
        "hysteria://", "hysteria2://", "hy2://", "tuic://",
        "anytls://", "wireguard://", "wg://", "ssh://",
    ];
    SCHEMES.iter().any(|p| s.contains(p))
}

fn looks_like_base64(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 16 {
        return false;
    }
    s.chars()
        .filter(|c| !c.is_whitespace())
        .take(256)
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '-' || c == '_' || c == '=')
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine};

    #[test]
    fn detects_uri_list() {
        let body = b"vless://abc@1.2.3.4:443\nvmess://xxx\n";
        assert_eq!(detect(body), SubscriptionFormat::UriList);
    }

    #[test]
    fn detects_base64() {
        let inner = "vless://abc@1.2.3.4:443\nvmess://xxx\n";
        let b = general_purpose::STANDARD.encode(inner);
        assert_eq!(detect(b.as_bytes()), SubscriptionFormat::Base64UriList);
    }

    #[test]
    fn detects_singbox_json() {
        let body = br#"{"log":{"level":"info"},"outbounds":[]}"#;
        assert_eq!(detect(body), SubscriptionFormat::SingBoxJson);
    }

    #[test]
    fn detects_clash() {
        let body = b"port: 7890\nproxies:\n  - name: a\n";
        assert_eq!(detect(body), SubscriptionFormat::ClashYaml);
    }

    #[test]
    fn detects_html() {
        let body = b"<!DOCTYPE html><html><head></head><body></body></html>";
        assert_eq!(detect(body), SubscriptionFormat::Html);
    }
}
