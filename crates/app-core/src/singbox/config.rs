//! Build a complete, *minimal* sing-box config from a [`ProxyProfile`].
//!
//! Targets **sing-box 1.13.x** schema:
//!  - inbounds carry no `sniff`/`sniff_override_destination` (moved to `route.rules`)
//!  - no `direct`/`block`/`dns` *outbounds* (those are now rule actions:
//!    `direct`, `reject`, `hijack-dns`)
//!  - DNS servers use the typed schema (`type: "https"` etc.)

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::profile::{
    Hysteria2Obfs, Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings,
    Transport,
};

/// How v2pn intercepts traffic on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionMode {
    /// SOCKS+HTTP "mixed" inbound on 127.0.0.1.
    Proxy,
    /// Layer-3 TUN device. Requires elevated privileges on Windows (Wintun).
    Tun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionOptions {
    pub mode: ConnectionMode,
    pub mixed_port: u16,
    pub clash_api_port: u16,
    pub ipv6: bool,
    pub strict_dns: bool,
    pub tun_interface_name: String,
    /// ISO-3166 alpha-2 country codes whose domains and IP blocks should
    /// **skip** the VPN tunnel and go direct. The "anti-censorship VPN"
    /// pattern: a Russian user wants Sberbank, Госуслуги, Yandex to keep
    /// working at full RU-CDN speed without bank-fraud freeze; a Chinese
    /// user wants Alipay/WeChat to do the same; an Iranian user wants
    /// Snapp/Tap30. Driven by sing-box's public `geosite-<cc>` and
    /// `geoip-<cc>` rule-sets.
    ///
    /// Empty list = nothing bypassed = pure tunnel-everything mode.
    /// Default is `["ru"]` since most v2pn users are in Russia, but the
    /// UI surfaces the full picker.
    #[serde(default = "default_bypass_country_codes")]
    pub bypass_country_codes: Vec<String>,
    /// User-authored bypass rules. Each line is one rule, in any of:
    ///   - `example.com`         → exact domain match
    ///   - `*.example.com`       → suffix match
    ///   - `192.168.0.0/16`      → IPv4 CIDR
    ///   - `2001:db8::/32`       → IPv6 CIDR
    ///   - `1.2.3.4`             → single IP (treated as /32)
    /// Lines starting with `#` are comments. Blank lines ignored.
    #[serde(default)]
    pub custom_bypass_rules: Vec<String>,
    /// Backwards-compat for the previous `bypass_ru: bool`. Deserialised
    /// from old config files; migrated into `bypass_country_codes` at
    /// load time. Never written back. Hidden from serde so it doesn't
    /// pollute new state.
    #[serde(default, skip_serializing)]
    pub bypass_ru: Option<bool>,
}

fn default_bypass_country_codes() -> Vec<String> {
    vec!["ru".to_string()]
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            mode: ConnectionMode::Proxy,
            mixed_port: 7890,
            clash_api_port: 9090,
            ipv6: false,
            strict_dns: true,
            tun_interface_name: "v2pn-tun".to_string(),
            bypass_country_codes: default_bypass_country_codes(),
            custom_bypass_rules: Vec::new(),
            bypass_ru: None,
        }
    }
}

/// Build the sing-box config for a *single* profile.
///
/// Convenience wrapper around [`build_config_multi`] for paths that don't
/// have the full subscription handy.
pub fn build_config(profile: &ProxyProfile, opts: &ConnectionOptions) -> Value {
    build_config_multi(std::slice::from_ref(profile), &profile.id, opts)
}

/// Build a sing-box config that holds **every** server in the subscription
/// behind a `selector` outbound named `proxy`. The active server is chosen
/// up-front via `default_id`, and can be switched at runtime through the
/// clash API (`PUT /proxies/proxy {"name": "<tag>"}`) without restarting
/// sing-box, the Wintun adapter, or the system route table.
pub fn build_config_multi(
    profiles: &[ProxyProfile],
    default_id: &str,
    opts: &ConnectionOptions,
) -> Value {
    // Each profile becomes its own outbound, plus a selector that points at
    // all of them.
    let mut outbounds: Vec<Value> = Vec::with_capacity(profiles.len() + 2);

    // The selector goes first (sing-box requires it before its members on
    // some older config schemas — keeping the order doesn't hurt newer ones).
    let server_tags: Vec<String> = profiles.iter().map(|p| server_tag(&p.id)).collect();
    let default_tag = server_tag(default_id);

    outbounds.push(json!({
        "type": "selector",
        "tag": "proxy",
        "outbounds": server_tags.clone(),
        "default": default_tag,
        // Drop existing TCP/UDP sessions so the next packet immediately uses
        // the new outbound — without this, long-lived connections (e.g. an
        // open YouTube stream) would keep flowing through the previous
        // server until they close on their own.
        "interrupt_exist_connections": true
    }));

    for p in profiles {
        outbounds.push(build_outbound_with_tag(p, &server_tag(&p.id)));
    }

    outbounds.push(json!({ "type": "direct", "tag": "direct" }));

    json!({
        "log": {
            "level": "info",
            "timestamp": true
        },
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{}", opts.clash_api_port),
                "default_mode": "rule"
            }
        },
        "dns": build_dns(opts),
        "inbounds": build_inbounds(opts),
        "outbounds": outbounds,
        "route": build_route(opts, profiles)
    })
}

/// Stable per-profile outbound tag. Uses the first 12 chars of the profile
/// id (UUID-ish) so collisions are practically impossible within one
/// subscription.
fn server_tag(profile_id: &str) -> String {
    let mut s = String::from("srv-");
    s.extend(profile_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(12));
    s
}

/// Public so commands can compute the same tag the config uses.
pub fn server_tag_for(profile_id: &str) -> String {
    server_tag(profile_id)
}

/* ============================================================ inbounds */

fn build_inbounds(opts: &ConnectionOptions) -> Value {
    let mut inbounds = vec![json!({
        "type": "mixed",
        "tag":  "mixed-in",
        "listen": "127.0.0.1",
        "listen_port": opts.mixed_port
    })];

    if matches!(opts.mode, ConnectionMode::Tun) {
        let mut tun = json!({
            "type": "tun",
            "tag":  "tun-in",
            "interface_name": opts.tun_interface_name,
            "address": ["172.19.0.1/30"],
            "auto_route": true,
            "strict_route": false,
            // "mixed": TCP via userspace gvisor stack, UDP via system stack.
            // Gvisor brings up far faster than the default "system" stack on
            // Windows (which has to wait for the kernel TCP/IP stack to settle
            // around the new adapter) and lowers per-packet latency for TCP.
            // We keep system for UDP because gvisor's UDP path has known
            // edge-cases with QUIC / Hysteria2 keepalives.
            "stack": "mixed",
            // Re-use already-bound endpoints when possible — saves a few ms
            // per new TCP flow at high session counts.
            "endpoint_independent_nat": true,
            // 1500 is the safe default; sing-box auto-clamps to interface MTU.
            "mtu": 1500
        });
        if opts.ipv6 {
            tun["address"] = json!(["172.19.0.1/30", "fdfe:dcba:9876::1/126"]);
        }
        inbounds.push(tun);
    }

    Value::Array(inbounds)
}

/* ============================================================ dns */

fn build_dns(opts: &ConnectionOptions) -> Value {
    let strategy = if opts.ipv6 { "prefer_ipv4" } else { "ipv4_only" };

    json!({
        "servers": [
            {
                "type": "https",
                "tag": "dns-proxy",
                "server": "1.1.1.1",
                "domain_resolver": "dns-direct",
                "detour": "proxy"
            },
            {
                "type": "https",
                "tag": "dns-direct",
                "server": "1.1.1.1"
            }
        ],
        "rules": [
            { "domain_suffix": [".lan", ".local"], "server": "dns-direct" }
        ],
        "final": if opts.strict_dns { "dns-proxy" } else { "dns-direct" },
        "strategy": strategy,
        "independent_cache": true
    })
}

/* ============================================================ route */

/// Split a list of profiles' `server` fields into bare IPs and hostnames.
/// IPs go into `ip_cidr`, hostnames into `domain`. Used to short-circuit
/// the proxy-server endpoints to the `direct` outbound — without this,
/// in TUN mode the new VLESS handshake initiated by `switch_server` gets
/// re-routed back into the TUN itself and dead-locks until the 5-second
/// dial timeout fires. The `process_name: ["sing-box.exe"]` rule is
/// supposed to handle this, but on Windows the process matcher does not
/// always tag connections opened from sing-box's own runtime — IP/domain
/// matching is the only deterministic fix.
fn collect_server_endpoints(profiles: &[ProxyProfile]) -> (Vec<String>, Vec<String>) {
    use std::collections::BTreeSet;
    use std::net::IpAddr;
    use std::str::FromStr;

    let mut ips: BTreeSet<String> = BTreeSet::new();
    let mut hosts: BTreeSet<String> = BTreeSet::new();

    for p in profiles {
        let server = p.server.trim();
        if server.is_empty() {
            continue;
        }
        match IpAddr::from_str(server) {
            Ok(IpAddr::V4(v4)) => {
                ips.insert(format!("{v4}/32"));
            }
            Ok(IpAddr::V6(v6)) => {
                ips.insert(format!("{v6}/128"));
            }
            Err(_) => {
                hosts.insert(server.to_lowercase());
            }
        }
    }

    (ips.into_iter().collect(), hosts.into_iter().collect())
}

fn build_route(opts: &ConnectionOptions, profiles: &[ProxyProfile]) -> Value {
    let (server_ips, server_hosts) = collect_server_endpoints(profiles);

    let mut rules = vec![
        // 1. Sniff TLS/HTTP/QUIC so we can route by domain — replaces the
        //    legacy `inbound.sniff: true` field that 1.13 removed.
        json!({ "action": "sniff" }),

        // 2. Hijack DNS so the kernel-level resolver runs through our
        //    `dns:` block — replaces the legacy `outbound: "dns"` route.
        json!({ "protocol": "dns", "action": "hijack-dns" }),
    ];

    // 3. Force-direct every proxy-server endpoint by IP and by hostname.
    //    This MUST come before the `process_name` bypass and before the
    //    private-IP rule: it's the only deterministic way to keep the
    //    sing-box ↔ upstream handshake out of the TUN once TUN is live.
    //    Critical for hot-switch via clash_api: without this, `switch_server`
    //    targeting a server whose IP wasn't yet seen by the route layer
    //    would dead-lock on an i/o timeout instead of working instantly.
    if !server_ips.is_empty() {
        rules.push(json!({ "ip_cidr": server_ips, "outbound": "direct" }));
    }
    if !server_hosts.is_empty() {
        rules.push(json!({ "domain": server_hosts, "outbound": "direct" }));
    }

    // 4. RFC1918 / link-local stays direct.
    rules.push(json!({ "ip_is_private": true, "outbound": "direct" }));

    if matches!(opts.mode, ConnectionMode::Tun) {
        // Belt-and-braces: bypass the sing-box process itself. Redundant
        // with the IP/domain rules above but cheap to keep, and serves as
        // a fallback if the user's subscription rotates servers without a
        // config rebuild.
        rules.push(json!({ "process_name": ["sing-box.exe"], "outbound": "direct" }));
    }

    // 5. Country-bypass + custom user rules ("anti-censorship VPN"
    // mode generalised). Per-country sing-box rule-sets short-circuit
    // matching domains/IPs to `direct`. User-supplied custom rules sit
    // alongside, parsed into the appropriate sing-box matcher
    // (`domain_suffix`, `domain`, `ip_cidr`).
    let mut rule_set = Vec::<Value>::new();
    let mut country_rule_set_tags: Vec<String> = Vec::new();

    for raw in &opts.bypass_country_codes {
        let cc = raw.trim().to_ascii_lowercase();
        // ISO-3166 alpha-2 only — defend the URL builder against
        // accidental injection from older state files.
        if cc.len() != 2 || !cc.chars().all(|c| c.is_ascii_lowercase()) {
            tracing::warn!(target: "singbox::config",
                "ignoring non-ISO bypass country code: {raw:?}");
            continue;
        }
        let geosite_tag = format!("geosite-{cc}");
        let geoip_tag = format!("geoip-{cc}");
        // SagerNet's `sing-geosite` rule-set branch only publishes a
        // bare `geosite-cn.srs` for China — every other country lives
        // under the `category-<cc>` namespace (e.g.
        // `geosite-category-ru.srs`, `geosite-category-ir.srs`).
        // Building `geosite-{cc}.srs` for non-CN yields a 404, which
        // sing-box then surfaces as `initial rule-set: ...: unexpected
        // status: 404 Not Found` and aborts startup.
        let geosite_file = if cc == "cn" {
            "geosite-cn".to_string()
        } else {
            format!("geosite-category-{cc}")
        };
        rule_set.push(json!({
            "tag": geosite_tag,
            "type": "remote",
            "format": "binary",
            // SagerNet's curated per-country site list — covers what
            // matters in that locale (banks, gov, top media, top
            // e-commerce). Updates weekly; sing-box auto-refreshes.
            "url": format!(
                "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/{geosite_file}.srs"
            ),
            "download_detour": "direct",
            "update_interval": "7d"
        }));
        rule_set.push(json!({
            "tag": geoip_tag,
            "type": "remote",
            "format": "binary",
            "url": format!(
                "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-{cc}.srs"
            ),
            "download_detour": "direct",
            "update_interval": "7d"
        }));
        country_rule_set_tags.push(geosite_tag);
        country_rule_set_tags.push(geoip_tag);
    }

    if !country_rule_set_tags.is_empty() {
        rules.push(json!({
            "rule_set": country_rule_set_tags,
            "outbound": "direct"
        }));
    }

    // Custom rules: user-authored lines parsed into discrete rule
    // entries. We sort each line into one of three buckets (domain
    // exact, domain suffix, ip_cidr) and emit at most one rule per
    // bucket so sing-box loads efficiently.
    let parsed = parse_custom_bypass_rules(&opts.custom_bypass_rules);
    if !parsed.exact_domains.is_empty() {
        rules.push(json!({ "domain": parsed.exact_domains, "outbound": "direct" }));
    }
    if !parsed.domain_suffixes.is_empty() {
        rules.push(json!({ "domain_suffix": parsed.domain_suffixes, "outbound": "direct" }));
    }
    if !parsed.ip_cidrs.is_empty() {
        rules.push(json!({ "ip_cidr": parsed.ip_cidrs, "outbound": "direct" }));
    }
    let mut out = json!({
        "auto_detect_interface": true,
        "default_domain_resolver": "dns-direct",
        "rules": rules,
        "final": "proxy"
    });
    if !rule_set.is_empty() {
        out.as_object_mut()
            .expect("route is object")
            .insert("rule_set".into(), Value::Array(rule_set));
    }
    out
}

/// Bucketised parsed result. Public so the UI can preview what its
/// custom rules would compile into before pressing Save.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct CustomBypassPub {
    pub exact_domains: Vec<String>,
    pub domain_suffixes: Vec<String>,
    pub ip_cidrs: Vec<String>,
}

/// Parse the user's free-form bypass rules into typed buckets. Tolerant
/// to whitespace, comments, blank lines, and the leading `*.` /
/// trailing `/` users habitually paste. Anything we can't make sense of
/// is silently dropped.
pub fn parse_custom_bypass_rules(lines: &[String]) -> CustomBypassPub {
    use std::net::IpAddr;
    use std::str::FromStr;

    let mut out = CustomBypassPub::default();
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Bare IP → /32 or /128
        if let Ok(ip) = IpAddr::from_str(line) {
            let cidr = match ip {
                IpAddr::V4(v4) => format!("{v4}/32"),
                IpAddr::V6(v6) => format!("{v6}/128"),
            };
            out.ip_cidrs.push(cidr);
            continue;
        }
        // CIDR
        if let Some((ip_part, mask_part)) = line.split_once('/') {
            if IpAddr::from_str(ip_part).is_ok() && mask_part.parse::<u8>().is_ok() {
                out.ip_cidrs.push(line.to_string());
                continue;
            }
        }
        // Wildcard suffix `*.example.com`
        if let Some(stripped) = line.strip_prefix("*.") {
            if !stripped.is_empty() {
                out.domain_suffixes.push(stripped.to_string());
                continue;
            }
        }
        // Bare suffix shorthand `.ozon.com`
        if let Some(stripped) = line.strip_prefix('.') {
            if !stripped.is_empty() {
                out.domain_suffixes.push(stripped.to_string());
                continue;
            }
        }
        // Otherwise treat as an exact domain. Lower-cased so the
        // sing-box matcher (case-sensitive) doesn't miss user-typed
        // capital letters.
        out.exact_domains.push(line.to_ascii_lowercase());
    }
    out
}

/* ============================================================ outbound */

/// Convert a `ProxyProfile` into a single sing-box outbound. The caller
/// supplies the `tag` because in multi-profile configs every server gets a
/// unique tag, while the legacy single-profile path uses `tag: "proxy"`.
pub fn build_outbound(p: &ProxyProfile) -> Value {
    build_outbound_with_tag(p, "proxy")
}

fn build_outbound_with_tag(p: &ProxyProfile, tag: &str) -> Value {
    let mut ob = json!({
        "tag": tag,
        "server": p.server,
        "server_port": p.port,
    });

    match (&p.protocol, &p.settings) {
        (Protocol::Vless, ProtocolSettings::Vless { uuid, flow }) => {
            ob["type"] = json!("vless");
            ob["uuid"] = json!(uuid);
            if let Some(flow) = flow {
                ob["flow"] = json!(flow);
            }
        }
        (Protocol::Vmess, ProtocolSettings::Vmess { uuid, alter_id, security }) => {
            ob["type"] = json!("vmess");
            ob["uuid"] = json!(uuid);
            ob["alter_id"] = json!(alter_id);
            ob["security"] = json!(security);
        }
        (Protocol::Trojan, ProtocolSettings::Trojan { password }) => {
            ob["type"] = json!("trojan");
            ob["password"] = json!(password);
        }
        (Protocol::Shadowsocks, ProtocolSettings::Shadowsocks { method, password }) => {
            ob["type"] = json!("shadowsocks");
            ob["method"] = json!(method);
            ob["password"] = json!(password);
        }
        (Protocol::Hysteria2, ProtocolSettings::Hysteria2 { password, obfs }) => {
            ob["type"] = json!("hysteria2");
            ob["password"] = json!(password);
            if let Some(Hysteria2Obfs { kind, password }) = obfs {
                ob["obfs"] = json!({ "type": kind, "password": password });
            }
        }
        (Protocol::Tuic, ProtocolSettings::Tuic { uuid, password, congestion_control }) => {
            ob["type"] = json!("tuic");
            ob["uuid"] = json!(uuid);
            ob["password"] = json!(password);
            if let Some(cc) = congestion_control {
                ob["congestion_control"] = json!(cc);
            }
        }
        _ => {
            return json!({ "type": "direct", "tag": tag });
        }
    }

    if let Some(t) = build_transport(&p.transport) {
        ob["transport"] = t;
    }
    if let Some(tls) = build_tls(&p.tls) {
        ob["tls"] = tls;
    }

    ob
}

fn build_transport(t: &Transport) -> Option<Value> {
    match t {
        Transport::Tcp => None,
        Transport::Ws { path, host, headers } => {
            let mut hdrs = serde_json::Map::new();
            for (k, v) in headers {
                hdrs.insert(k.clone(), Value::String(v.clone()));
            }
            if let Some(h) = host {
                hdrs.insert("Host".into(), Value::String(h.clone()));
            }
            Some(json!({
                "type": "ws",
                "path": path,
                "headers": Value::Object(hdrs),
            }))
        }
        Transport::Grpc { service_name } => Some(json!({
            "type": "grpc",
            "service_name": service_name,
        })),
        Transport::HttpUpgrade { path, host } => {
            let mut v = json!({ "type": "httpupgrade", "path": path });
            if let Some(h) = host {
                v["host"] = json!(h);
            }
            Some(v)
        }
        Transport::XHttp { path, mode } => {
            let mut v = json!({ "type": "http", "path": path });
            if let Some(m) = mode {
                v["method"] = json!(m);
            }
            Some(v)
        }
        Transport::Quic => None,
    }
}

fn build_tls(tls: &TlsSettings) -> Option<Value> {
    if !tls.enabled {
        return None;
    }
    let mut v = json!({ "enabled": true, "insecure": tls.allow_insecure });
    if let Some(sn) = &tls.server_name {
        v["server_name"] = json!(sn);
    }
    if !tls.alpn.is_empty() {
        v["alpn"] = json!(tls.alpn);
    }
    if let Some(fp) = &tls.utls_fingerprint {
        v["utls"] = json!({ "enabled": true, "fingerprint": fp });
    }
    if let Some(RealitySettings { public_key, short_id, .. }) = &tls.reality {
        let mut r = json!({ "enabled": true, "public_key": public_key });
        if let Some(sid) = short_id {
            r["short_id"] = json!(sid);
        }
        v["reality"] = r;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscription::uri::parse_uri;

    fn vless_reality() -> ProxyProfile {
        parse_uri(
            "vless://550e8400-e29b-41d4-a716-446655440000@example.com:443\
             ?type=tcp&security=reality&pbk=AAAA&fp=chrome&sni=www.cloudflare.com\
             &sid=12&flow=xtls-rprx-vision#NL%201",
        )
        .unwrap()
    }

    #[test]
    fn proxy_mode_has_only_mixed_inbound() {
        let cfg = build_config(&vless_reality(), &ConnectionOptions::default());
        let inbounds = cfg["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 1);
        assert_eq!(inbounds[0]["type"], "mixed");
        assert_eq!(inbounds[0]["listen"], "127.0.0.1");
        // Legacy field must not be set in 1.13 schema.
        assert!(inbounds[0].get("sniff").is_none());
    }

    #[test]
    fn tun_mode_adds_tun_inbound() {
        let opts = ConnectionOptions {
            mode: ConnectionMode::Tun,
            ..Default::default()
        };
        let cfg = build_config(&vless_reality(), &opts);
        let inbounds = cfg["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 2);
        assert_eq!(inbounds[1]["type"], "tun");
        assert_eq!(inbounds[1]["auto_route"], true);
    }

    #[test]
    fn outbound_carries_reality_and_vision() {
        let cfg = build_config(&vless_reality(), &ConnectionOptions::default());
        // build_config now delegates to build_config_multi, which puts the
        // selector first. Find the actual vless outbound by type, not index.
        let outbounds = cfg["outbounds"].as_array().unwrap();
        let proxy = outbounds
            .iter()
            .find(|o| o["type"] == "vless")
            .expect("vless outbound missing");
        assert_eq!(proxy["flow"], "xtls-rprx-vision");
        assert_eq!(proxy["tls"]["server_name"], "www.cloudflare.com");
        assert_eq!(proxy["tls"]["reality"]["public_key"], "AAAA");
        assert_eq!(proxy["tls"]["utls"]["fingerprint"], "chrome");
    }

    #[test]
    fn route_uses_rule_actions() {
        let cfg = build_config(&vless_reality(), &ConnectionOptions::default());
        let rules = cfg["route"]["rules"].as_array().unwrap();
        // First rule must be the sniff action.
        assert_eq!(rules[0]["action"], "sniff");
        // hijack-dns must be present somewhere.
        assert!(rules.iter().any(|r| r["action"] == "hijack-dns"));
        assert_eq!(cfg["route"]["final"], "proxy");
    }

    #[test]
    fn no_legacy_outbounds() {
        let cfg = build_config(&vless_reality(), &ConnectionOptions::default());
        let outbounds = cfg["outbounds"].as_array().unwrap();
        for ob in outbounds {
            let ty = ob["type"].as_str().unwrap_or("");
            assert!(
                ty != "block" && ty != "dns",
                "legacy outbound type still present: {ty}"
            );
        }
    }

    #[test]
    fn server_endpoints_are_routed_direct() {
        // Mix of IP and hostname servers — both must show up in the route
        // bypass rules so hot-switch in TUN mode never dead-locks.
        let mut by_ip = vless_reality();
        by_ip.id = "ip-srv-0001".into();
        by_ip.server = "37.9.4.207".into();

        let mut by_host = vless_reality();
        by_host.id = "host-srv-0001".into();
        by_host.server = "ll.astrokolchick.com".into();

        let opts = ConnectionOptions {
            mode: ConnectionMode::Tun,
            ..Default::default()
        };
        let cfg = build_config_multi(&[by_ip, by_host], "ip-srv-0001", &opts);
        let rules = cfg["route"]["rules"].as_array().unwrap();

        // Assert that *some* rule directs `37.9.4.207/32` to `direct`.
        let has_ip_rule = rules.iter().any(|r| {
            r["outbound"] == "direct"
                && r["ip_cidr"]
                    .as_array()
                    .map(|a| a.iter().any(|v| v == "37.9.4.207/32"))
                    .unwrap_or(false)
        });
        assert!(has_ip_rule, "IP-server bypass missing: {rules:#?}");

        // And one rule directs `ll.astrokolchick.com` to `direct`.
        let has_host_rule = rules.iter().any(|r| {
            r["outbound"] == "direct"
                && r["domain"]
                    .as_array()
                    .map(|a| a.iter().any(|v| v == "ll.astrokolchick.com"))
                    .unwrap_or(false)
        });
        assert!(has_host_rule, "domain-server bypass missing: {rules:#?}");
    }

    #[test]
    fn server_bypass_runs_before_proxy_final() {
        // Ordering matters: the bypass must hit before any rule that would
        // send these endpoints back through `proxy`. We assert the bypass
        // is present and `final` is still `proxy`.
        let mut p = vless_reality();
        p.server = "1.2.3.4".into();
        let cfg = build_config_multi(&[p], "550e8400-e29b-41d4-a716-446655440000", &ConnectionOptions::default());
        let rules = cfg["route"]["rules"].as_array().unwrap();

        // sniff first, then hijack-dns, then bypass IPs.
        assert_eq!(rules[0]["action"], "sniff");
        assert_eq!(rules[1]["action"], "hijack-dns");
        let third = &rules[2];
        assert_eq!(third["outbound"], "direct");
        assert!(third["ip_cidr"].is_array());
    }

    #[test]
    fn geosite_rule_set_url_uses_category_prefix_for_non_cn() {
        // Regression: `https://raw.githubusercontent.com/SagerNet/sing-geosite/
        // rule-set/geosite-ru.srs` is a 404. Russia (and every other non-CN
        // country) lives under the `category-<cc>` namespace; only China has
        // a top-level `geosite-cn.srs`. If this drifts back to the old
        // `geosite-{cc}` shape, sing-box aborts startup with
        // "initial rule-set: geosite-ru: unexpected status: 404 Not Found".
        let opts = ConnectionOptions {
            bypass_country_codes: vec!["ru".into(), "cn".into(), "ir".into()],
            ..Default::default()
        };
        let cfg = build_config(&vless_reality(), &opts);
        let rs = cfg["route"]["rule_set"].as_array().expect("rule_set array");

        let url_for = |tag: &str| -> String {
            rs.iter()
                .find(|e| e["tag"] == tag)
                .unwrap_or_else(|| panic!("missing rule_set entry {tag}: {rs:#?}"))
                ["url"]
                .as_str()
                .expect("url is string")
                .to_string()
        };

        assert_eq!(
            url_for("geosite-ru"),
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ru.srs",
        );
        assert_eq!(
            url_for("geosite-ir"),
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ir.srs",
        );
        // CN is the special case — there is no `geosite-category-cn.srs`.
        assert_eq!(
            url_for("geosite-cn"),
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
        );
        // geoip URLs were already correct; lock them in too.
        assert_eq!(
            url_for("geoip-ru"),
            "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ru.srs",
        );
    }
}
