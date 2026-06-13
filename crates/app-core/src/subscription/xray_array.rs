//! Parser for the **Happ-style array of Xray configs** subscription format.
//!
//! Used by Remnawave/BuzzVPN-style panels when serving subscriptions to the
//! Happ client. Wire format:
//!
//! ```json
//! [
//!   { "remarks": "🇺🇸 USA",       "outbounds": [{...vless...}, ...service...] },
//!   { "remarks": "🇩🇪 Germany",   "outbounds": [{...trojan...}] },
//!   ...
//! ]
//! ```
//!
//! Each top-level element is one user-visible "server entry". `remarks`
//! becomes the display name; the first `outbounds[]` entry whose protocol is
//! a real proxy (vless/vmess/trojan/ss/hysteria2/tuic/wireguard) becomes the
//! profile.
//!
//! Service outbounds (`direct`/`block`/`dns`/`blackhole`) and selector-style
//! load-balancers are skipped — they reflect the panel's internal routing,
//! not something the user picks.

use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::profile::{
    Hysteria2Obfs, Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings,
    Transport,
};

/// Returns true if the body looks like a top-level JSON array of Xray configs.
pub fn looks_like(body: &[u8]) -> bool {
    let head: String = body
        .iter()
        .take(8192)
        .map(|&b| if b.is_ascii() { b as char } else { ' ' })
        .collect();
    let trimmed = head.trim_start();
    if !trimmed.starts_with('[') {
        return false;
    }
    let lc = trimmed.to_ascii_lowercase();
    // The signature of v2rayN/Happ Xray client subscriptions is a top-
    // level JSON array whose first element carries an `outbounds` array.
    // Earlier we additionally required `remarks` to appear in the head,
    // but that broke real-world panels (Buzzvpn ships ~28 outbounds per
    // entry, ~3 KiB of `inbounds` first, so the `remarks` key for the
    // *first* entry sits past the 4 KiB sniff window). `outbounds` alone
    // is a strong enough fingerprint: nothing else legitimate ships a
    // top-level JSON array with that key as far as we've seen.
    lc.contains("\"outbounds\"")
}

pub fn parse(body: &[u8]) -> CoreResult<Vec<ProxyProfile>> {
    let arr: Vec<Value> = serde_json::from_slice(body)
        .map_err(|e| CoreError::Parse(format!("xray-array: {e}")))?;

    let mut out = Vec::with_capacity(arr.len());
    for entry in arr {
        let remarks = entry
            .get("remarks")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let Some(outbounds) = entry.get("outbounds").and_then(|v| v.as_array()) else {
            continue;
        };

        if let Some(profile) = pick_real_outbound(outbounds, &remarks) {
            out.push(profile);
        }
    }
    Ok(out)
}

fn pick_real_outbound(outbounds: &[Value], name: &str) -> Option<ProxyProfile> {
    for ob in outbounds {
        let proto = ob.get("protocol").and_then(|v| v.as_str()).unwrap_or("");
        if !is_real_proxy(proto) {
            continue;
        }
        if let Some(p) = parse_xray_outbound(ob, name) {
            return Some(p);
        }
    }
    None
}

fn is_real_proxy(p: &str) -> bool {
    matches!(
        p,
        "vless" | "vmess" | "trojan" | "shadowsocks"
        | "hysteria2" | "tuic" | "wireguard"
    )
}

fn parse_xray_outbound(ob: &Value, name: &str) -> Option<ProxyProfile> {
    let proto = ob.get("protocol")?.as_str()?;
    let settings = ob.get("settings")?;
    let stream = ob.get("streamSettings");

    // Address & port live under `vnext` (vless/vmess) or `servers` (trojan/ss/hy2/tuic).
    let (server, port, secret_a, secret_b, alter_id, security) = match proto {
        "vless" | "vmess" => {
            let v = settings.get("vnext")?.as_array()?.first()?;
            let server = v.get("address")?.as_str()?.to_string();
            let port = v.get("port")?.as_u64()? as u16;
            let user = v.get("users")?.as_array()?.first()?;
            let id = user.get("id")?.as_str()?.to_string();
            let alter = user.get("alterId").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let sec = user
                .get("security")
                .and_then(|x| x.as_str())
                .unwrap_or("auto")
                .to_string();
            (server, port, id, String::new(), alter, sec)
        }
        "trojan" | "shadowsocks" | "hysteria2" | "tuic" => {
            let s = settings.get("servers")?.as_array()?.first()?;
            let server = s.get("address")?.as_str()?.to_string();
            let port = s.get("port")?.as_u64()? as u16;
            let pwd = s
                .get("password")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let method = s
                .get("method")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (server, port, pwd, method, 0, String::new())
        }
        _ => return None,
    };

    let proto_settings = match proto {
        "vless" => {
            let flow = settings
                .get("vnext").and_then(|v| v.as_array()).and_then(|a| a.first())
                .and_then(|v| v.get("users")).and_then(|u| u.as_array()).and_then(|a| a.first())
                .and_then(|u| u.get("flow")).and_then(|x| x.as_str()).map(str::to_string);
            ProtocolSettings::Vless { uuid: secret_a, flow }
        }
        "vmess" => ProtocolSettings::Vmess {
            uuid: secret_a,
            alter_id,
            security,
        },
        "trojan" => ProtocolSettings::Trojan { password: secret_a },
        "shadowsocks" => ProtocolSettings::Shadowsocks {
            method: secret_b,
            password: secret_a,
        },
        "hysteria2" => ProtocolSettings::Hysteria2 {
            password: secret_a,
            obfs: settings
                .pointer("/servers/0/obfs")
                .and_then(|o| {
                    Some(Hysteria2Obfs {
                        kind: o.get("type")?.as_str()?.to_string(),
                        password: o.get("password")?.as_str()?.to_string(),
                    })
                }),
        },
        "tuic" => ProtocolSettings::Tuic {
            uuid: secret_a,
            password: secret_b,
            congestion_control: settings
                .pointer("/servers/0/congestion_control")
                .and_then(|x| x.as_str()).map(str::to_string),
        },
        _ => return None,
    };

    let protocol = match proto {
        "vless" => Protocol::Vless,
        "vmess" => Protocol::Vmess,
        "trojan" => Protocol::Trojan,
        "shadowsocks" => Protocol::Shadowsocks,
        "hysteria2" => Protocol::Hysteria2,
        "tuic" => Protocol::Tuic,
        _ => return None,
    };

    let transport = parse_transport(stream);
    let tls = parse_tls(stream);

    Some(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: name.to_string(),
        country_code: None,
        protocol,
        server,
        port,
        settings: proto_settings,
        transport,
        tls,
        subscription_id: None,
    })
}

fn parse_transport(stream: Option<&Value>) -> Transport {
    let Some(s) = stream else { return Transport::Tcp };
    let net = s.get("network").and_then(|x| x.as_str()).unwrap_or("tcp");
    match net {
        "ws" => {
            let opts = s.get("wsSettings");
            Transport::Ws {
                path: opts
                    .and_then(|o| o.get("path"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("/").to_string(),
                host: opts
                    .and_then(|o| o.get("headers"))
                    .and_then(|h| h.get("Host"))
                    .and_then(|x| x.as_str()).map(str::to_string),
                headers: vec![],
            }
        }
        "grpc" => Transport::Grpc {
            service_name: s
                .get("grpcSettings")
                .and_then(|g| g.get("serviceName"))
                .and_then(|x| x.as_str())
                .unwrap_or("").to_string(),
        },
        "httpupgrade" => Transport::HttpUpgrade {
            path: s
                .get("httpupgradeSettings")
                .and_then(|h| h.get("path"))
                .and_then(|x| x.as_str())
                .unwrap_or("/").to_string(),
            host: s
                .get("httpupgradeSettings")
                .and_then(|h| h.get("host"))
                .and_then(|x| x.as_str()).map(str::to_string),
        },
        _ => Transport::Tcp,
    }
}

fn parse_tls(stream: Option<&Value>) -> TlsSettings {
    let Some(s) = stream else { return TlsSettings::default() };
    let security = s
        .get("security")
        .and_then(|x| x.as_str())
        .unwrap_or("none");
    let enabled = matches!(security, "tls" | "reality" | "xtls");

    let (server_name, alpn, fp, allow_insecure) = if security == "reality" {
        let r = s.get("realitySettings");
        (
            r.and_then(|x| x.get("serverName")).and_then(|x| x.as_str()).map(str::to_string),
            r.and_then(|x| x.get("alpn")).and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            r.and_then(|x| x.get("fingerprint")).and_then(|x| x.as_str()).map(str::to_string),
            false,
        )
    } else {
        let t = s.get("tlsSettings");
        (
            t.and_then(|x| x.get("serverName")).and_then(|x| x.as_str()).map(str::to_string),
            t.and_then(|x| x.get("alpn")).and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            t.and_then(|x| x.get("fingerprint")).and_then(|x| x.as_str()).map(str::to_string),
            t.and_then(|x| x.get("allowInsecure")).and_then(|x| x.as_bool()).unwrap_or(false),
        )
    };

    let reality = if security == "reality" {
        s.get("realitySettings").map(|r| RealitySettings {
            public_key: r.get("publicKey").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            short_id: r.get("shortId").and_then(|x| x.as_str()).map(str::to_string),
            spider_x: r.get("spiderX").and_then(|x| x.as_str()).map(str::to_string),
        })
    } else {
        None
    };

    TlsSettings {
        enabled,
        server_name,
        alpn,
        allow_insecure,
        utls_fingerprint: fp,
        reality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_format() {
        assert!(looks_like(br#"[{"remarks":"x","outbounds":[]}]"#));
        assert!(!looks_like(br#"{"outbounds":[]}"#));
        assert!(!looks_like(b"vless://abc"));
    }

    #[test]
    fn parses_real_buzz_shape() {
        let body = br#"[
          {
            "remarks": "USA",
            "outbounds": [
              {"tag":"block","protocol":"blackhole"},
              {
                "tag":"proxy","protocol":"vless",
                "settings":{"vnext":[{"address":"1.2.3.4","port":443,
                  "users":[{"id":"550e8400-e29b-41d4-a716-446655440000",
                    "encryption":"none","flow":"xtls-rprx-vision"}]}]},
                "streamSettings":{"network":"raw","security":"reality",
                  "realitySettings":{"serverName":"www.cloudflare.com",
                    "publicKey":"AAAA","shortId":"12","fingerprint":"chrome"}}
              }
            ]
          }
        ]"#;
        let v = parse(body).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "USA");
        assert_eq!(v[0].server, "1.2.3.4");
        assert_eq!(v[0].port, 443);
        assert!(matches!(v[0].protocol, Protocol::Vless));
        assert!(v[0].tls.reality.is_some());
        assert_eq!(v[0].tls.utls_fingerprint.as_deref(), Some("chrome"));
    }
}
