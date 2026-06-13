//! Minimal sing-box JSON parser → internal profiles.
//!
//! We accept the full sing-box config and walk `outbounds[]`, mapping each
//! supported outbound to a [`ProxyProfile`]. Unknown outbound types are
//! silently skipped (e.g. `direct`, `block`, `dns`, `selector`, `urltest`).
//!
//! This implementation is deliberately tolerant: we never fail the whole
//! subscription because of one malformed server.

use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::profile::{
    Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings, Transport,
};

pub fn parse(body: &[u8]) -> CoreResult<Vec<ProxyProfile>> {
    let v: Value = serde_json::from_slice(body)?;
    let outbounds = v
        .get("outbounds")
        .and_then(|o| o.as_array())
        .ok_or_else(|| CoreError::Parse("sing-box: outbounds[] missing".into()))?;

    let mut out = Vec::with_capacity(outbounds.len());
    for ob in outbounds {
        if let Some(p) = parse_outbound(ob) {
            out.push(p);
        }
    }
    Ok(out)
}

fn parse_outbound(ob: &Value) -> Option<ProxyProfile> {
    let ty = ob.get("type")?.as_str()?;
    let tag = ob.get("tag").and_then(|t| t.as_str()).unwrap_or("");

    let server = ob.get("server").and_then(|x| x.as_str())?.to_string();
    let port = ob
        .get("server_port")
        .and_then(|x| x.as_u64())
        .map(|p| p as u16)?;

    let (protocol, settings) = match ty {
        "vless" => {
            let uuid = ob.get("uuid")?.as_str()?.to_string();
            let flow = ob.get("flow").and_then(|x| x.as_str()).map(str::to_string);
            (Protocol::Vless, ProtocolSettings::Vless { uuid, flow })
        }
        "vmess" => {
            let uuid = ob.get("uuid")?.as_str()?.to_string();
            let alter_id = ob.get("alter_id").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let security = ob
                .get("security")
                .and_then(|x| x.as_str())
                .unwrap_or("auto")
                .to_string();
            (
                Protocol::Vmess,
                ProtocolSettings::Vmess { uuid, alter_id, security },
            )
        }
        "trojan" => {
            let password = ob.get("password")?.as_str()?.to_string();
            (Protocol::Trojan, ProtocolSettings::Trojan { password })
        }
        "shadowsocks" => {
            let method = ob.get("method")?.as_str()?.to_string();
            let password = ob.get("password")?.as_str()?.to_string();
            (
                Protocol::Shadowsocks,
                ProtocolSettings::Shadowsocks { method, password },
            )
        }
        "hysteria2" => {
            let password = ob
                .get("password")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (
                Protocol::Hysteria2,
                ProtocolSettings::Hysteria2 { password, obfs: None },
            )
        }
        "tuic" => {
            let uuid = ob.get("uuid")?.as_str()?.to_string();
            let password = ob
                .get("password")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (
                Protocol::Tuic,
                ProtocolSettings::Tuic {
                    uuid,
                    password,
                    congestion_control: ob
                        .get("congestion_control")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                },
            )
        }
        // selector / urltest / direct / block / dns: not a server, skip
        _ => return None,
    };

    let transport = parse_transport(ob.get("transport"));
    let tls = parse_tls(ob.get("tls"));

    Some(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: tag.to_string(),
        country_code: None,
        protocol,
        server,
        port,
        settings,
        transport,
        tls,
        subscription_id: None,
    })
}

fn parse_transport(v: Option<&Value>) -> Transport {
    let Some(v) = v else { return Transport::Tcp };
    let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match ty {
        "ws" => Transport::Ws {
            path: v
                .get("path")
                .and_then(|x| x.as_str())
                .unwrap_or("/")
                .to_string(),
            host: v.get("headers")
                .and_then(|h| h.get("Host"))
                .and_then(|x| x.as_str())
                .map(str::to_string),
            headers: vec![],
        },
        "grpc" => Transport::Grpc {
            service_name: v
                .get("service_name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        },
        "httpupgrade" => Transport::HttpUpgrade {
            path: v
                .get("path")
                .and_then(|x| x.as_str())
                .unwrap_or("/")
                .to_string(),
            host: v.get("host").and_then(|x| x.as_str()).map(str::to_string),
        },
        "quic" => Transport::Quic,
        _ => Transport::Tcp,
    }
}

fn parse_tls(v: Option<&Value>) -> TlsSettings {
    let Some(v) = v else { return TlsSettings::default() };
    let enabled = v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
    let server_name = v
        .get("server_name")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let alpn = v
        .get("alpn")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let allow_insecure = v
        .get("insecure")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let utls_fingerprint = v
        .get("utls")
        .and_then(|u| u.get("fingerprint"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let reality = v.get("reality").and_then(|r| {
        let enabled = r.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
        if !enabled {
            return None;
        }
        Some(RealitySettings {
            public_key: r
                .get("public_key")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            short_id: r
                .get("short_id")
                .and_then(|x| x.as_str())
                .map(str::to_string),
            spider_x: None,
        })
    });
    TlsSettings {
        enabled,
        server_name,
        alpn,
        allow_insecure,
        utls_fingerprint,
        reality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_singbox() {
        let body = br#"{
            "outbounds": [
                {"type":"direct","tag":"direct"},
                {
                    "type":"vless",
                    "tag":"NL 1",
                    "server":"example.com",
                    "server_port":443,
                    "uuid":"550e8400-e29b-41d4-a716-446655440000",
                    "flow":"xtls-rprx-vision",
                    "tls":{"enabled":true,"server_name":"www.cloudflare.com","reality":{"enabled":true,"public_key":"AAAA","short_id":"12"},"utls":{"enabled":true,"fingerprint":"chrome"}}
                }
            ]
        }"#;
        let v = parse(body).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "NL 1");
        assert_eq!(v[0].port, 443);
        assert_eq!(v[0].protocol, Protocol::Vless);
        assert!(v[0].tls.reality.is_some());
    }
}
