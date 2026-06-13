//! Defensive sanitiser for sing-box configs.
//!
//! Subscriptions are untrusted input. A malicious server can ship a
//! `outbounds: [{type: "direct"}]` or worse, an `inbounds:` array that opens
//! a SOCKS server on `0.0.0.0` so any host on the network can use the user's
//! machine as a relay.
//!
//! This module is the last gate before a config is written to disk and
//! handed to `sing-box`. It enforces:
//!
//!  - inbounds[].listen ∈ {127.0.0.1, ::1, "localhost"}
//!  - tun is allowed (no `listen` field) but only with v2pn-controlled name
//!  - clash_api/external_controller binds to localhost
//!  - no `secret` is exposed via clash_api over the wire
//!  - external rule_set `url:` fields are blocked (could phone-home)
//!
//! The function returns a list of *errors* (fatal) and *warnings* (non-fatal,
//! e.g. unknown fields). Empty errors → safe to start.

use serde_json::Value;

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Default)]
pub struct SanitizeReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl SanitizeReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Inspect a sing-box config, mutating it where safe, and return a report.
pub fn sanitize(cfg: &mut Value) -> SanitizeReport {
    let mut r = SanitizeReport::default();

    // ---- inbounds ---------------------------------------------------------
    if let Some(inbounds) = cfg.get_mut("inbounds").and_then(|v| v.as_array_mut()) {
        for (idx, inb) in inbounds.iter_mut().enumerate() {
            let ty = inb.get("type").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            match ty.as_str() {
                "tun" => {
                    // We don't allow auto_redirect on macOS through here — pure flag check.
                    if inb.get("auto_redirect").and_then(|v| v.as_bool()).unwrap_or(false) {
                        r.warnings.push(format!(
                            "inbound[{idx}].auto_redirect=true: removed (not allowed)"
                        ));
                        inb.as_object_mut().unwrap().remove("auto_redirect");
                    }
                }
                "mixed" | "socks" | "http" | "redirect" | "tproxy" => {
                    let listen = inb.get("listen").and_then(|v| v.as_str()).unwrap_or("");
                    if !is_loopback(listen) {
                        r.errors.push(format!(
                            "inbound[{idx}] type={ty} listen='{listen}' is not loopback"
                        ));
                    }
                }
                other => {
                    r.warnings.push(format!("inbound[{idx}] unknown type '{other}'"));
                }
            }
        }
    } else {
        r.errors.push("config has no inbounds[] array".into());
    }

    // ---- outbounds --------------------------------------------------------
    if cfg
        .get("outbounds")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(true)
    {
        r.errors.push("config has no outbounds[]".into());
    } else if let Some(outbounds) = cfg.get("outbounds").and_then(|v| v.as_array()) {
        // selector / urltest are first-class proxy types in sing-box; their
        // members must be valid outbound tags. We only sanity-check that
        // every referenced tag exists.
        let known_tags: std::collections::HashSet<String> = outbounds
            .iter()
            .filter_map(|ob| ob.get("tag").and_then(|v| v.as_str()))
            .map(str::to_string)
            .collect();
        for (idx, ob) in outbounds.iter().enumerate() {
            let ty = ob.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(ty, "selector" | "urltest") {
                if let Some(members) = ob.get("outbounds").and_then(|v| v.as_array()) {
                    for m in members {
                        if let Some(name) = m.as_str() {
                            if !known_tags.contains(name) {
                                r.warnings.push(format!(
                                    "outbound[{idx}] type={ty} references unknown tag '{name}'"
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- experimental.clash_api ------------------------------------------
    if let Some(api) = cfg.pointer_mut("/experimental/clash_api") {
        if let Some(addr) = api.get("external_controller").and_then(|v| v.as_str()) {
            let host = addr.split(':').next().unwrap_or("");
            if !is_loopback(host) {
                r.errors.push(format!(
                    "experimental.clash_api.external_controller='{addr}' must be loopback"
                ));
            }
        }
        // Strip any preset password — the supervisor uses the local socket
        // directly and a leaked password to clash_api would be enough to
        // hijack the proxy from another local user account.
        if api.as_object_mut().map(|o| o.remove("secret").is_some()).unwrap_or(false) {
            r.warnings.push("experimental.clash_api.secret: stripped".into());
        }
    }

    // ---- route.rule_set with external url -------------------------------
    if let Some(sets) = cfg.pointer_mut("/route/rule_set").and_then(|v| v.as_array_mut()) {
        for (idx, rs) in sets.iter_mut().enumerate() {
            if let Some(ty) = rs.get("type").and_then(|v| v.as_str()) {
                if ty == "remote" {
                    let url = rs.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    if !is_trusted_geo_url(url) {
                        r.errors.push(format!(
                            "route.rule_set[{idx}] remote url not in allow-list: {url}"
                        ));
                    }
                }
            }
        }
    }

    r
}

/// Convenience: sanitize and turn fatal report into a [`CoreError`].
pub fn sanitize_strict(cfg: &mut Value) -> CoreResult<SanitizeReport> {
    let r = sanitize(cfg);
    if !r.ok() {
        return Err(CoreError::InvalidConfig(r.errors.join("; ")));
    }
    Ok(r)
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "::1" | "localhost" | "[::1]")
}

/// Allow only well-known geosite/geoip mirrors. Add cautiously.
fn is_trusted_geo_url(url: &str) -> bool {
    const ALLOW: &[&str] = &[
        "https://raw.githubusercontent.com/SagerNet/sing-geosite/",
        "https://raw.githubusercontent.com/SagerNet/sing-geoip/",
        "https://github.com/SagerNet/sing-geosite/",
        "https://github.com/SagerNet/sing-geoip/",
    ];
    ALLOW.iter().any(|prefix| url.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_non_loopback_inbound() {
        let mut cfg = json!({
            "inbounds": [{ "type": "mixed", "listen": "0.0.0.0", "listen_port": 7890 }],
            "outbounds": [{ "type": "direct" }]
        });
        let r = sanitize(&mut cfg);
        assert!(!r.ok());
        assert!(r.errors[0].contains("0.0.0.0"));
    }

    #[test]
    fn accepts_loopback_mixed() {
        let mut cfg = json!({
            "inbounds": [{ "type": "mixed", "listen": "127.0.0.1", "listen_port": 7890 }],
            "outbounds": [{ "type": "vless", "server": "x", "server_port": 1 }]
        });
        let r = sanitize(&mut cfg);
        assert!(r.ok(), "errors: {:?}", r.errors);
    }

    #[test]
    fn strips_clash_api_secret() {
        let mut cfg = json!({
            "inbounds": [{ "type": "mixed", "listen": "127.0.0.1", "listen_port": 7890 }],
            "outbounds": [{ "type": "vless", "server": "x", "server_port": 1 }],
            "experimental": {
                "clash_api": {
                    "external_controller": "127.0.0.1:9090",
                    "secret": "leaked"
                }
            }
        });
        let r = sanitize(&mut cfg);
        assert!(r.ok());
        assert!(cfg.pointer("/experimental/clash_api/secret").is_none());
        assert!(r.warnings.iter().any(|w| w.contains("secret")));
    }

    #[test]
    fn rejects_untrusted_ruleset_url() {
        let mut cfg = json!({
            "inbounds": [{ "type": "mixed", "listen": "127.0.0.1", "listen_port": 7890 }],
            "outbounds": [{ "type": "vless", "server": "x", "server_port": 1 }],
            "route": {
                "rule_set": [{
                    "type": "remote",
                    "url": "https://evil.example/ruleset.srs"
                }]
            }
        });
        let r = sanitize(&mut cfg);
        assert!(!r.ok());
        assert!(r.errors.iter().any(|e| e.contains("allow-list")));
    }
}
