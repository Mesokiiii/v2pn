//! Clash / Clash.Meta YAML parser → internal profiles.
//!
//! Walks `proxies:` and maps each entry to a [`ProxyProfile`]. Like the sing-box
//! parser, malformed entries are skipped, not fatal.

use serde_yaml::Value;

use crate::error::CoreResult;
use crate::profile::{
    Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings, Transport,
};

pub fn parse(body: &[u8]) -> CoreResult<Vec<ProxyProfile>> {
    let doc: Value = serde_yaml::from_slice(body)?;
    let proxies = doc
        .get("proxies")
        .and_then(|p| p.as_sequence())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::with_capacity(proxies.len());
    for px in proxies {
        if let Some(p) = parse_proxy(&px) {
            out.push(p);
        }
    }
    Ok(out)
}

fn parse_proxy(v: &Value) -> Option<ProxyProfile> {
    let name = v.get("name")?.as_str()?.to_string();
    let ty = v.get("type")?.as_str()?;
    let server = v.get("server")?.as_str()?.to_string();
    let port = v.get("port")?.as_u64()? as u16;

    let (protocol, settings) = match ty {
        "vless" => {
            let uuid = v.get("uuid")?.as_str()?.to_string();
            let flow = v.get("flow").and_then(|x| x.as_str()).map(str::to_string);
            (Protocol::Vless, ProtocolSettings::Vless { uuid, flow })
        }
        "vmess" => {
            let uuid = v.get("uuid")?.as_str()?.to_string();
            let alter_id = v.get("alterId").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let security = v
                .get("cipher")
                .and_then(|x| x.as_str())
                .unwrap_or("auto")
                .to_string();
            (
                Protocol::Vmess,
                ProtocolSettings::Vmess { uuid, alter_id, security },
            )
        }
        "trojan" => {
            let password = v.get("password")?.as_str()?.to_string();
            (Protocol::Trojan, ProtocolSettings::Trojan { password })
        }
        "ss" | "shadowsocks" => {
            let method = v.get("cipher")?.as_str()?.to_string();
            let password = v.get("password")?.as_str()?.to_string();
            (
                Protocol::Shadowsocks,
                ProtocolSettings::Shadowsocks { method, password },
            )
        }
        "hysteria2" => {
            let password = v
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
            let uuid = v.get("uuid")?.as_str()?.to_string();
            let password = v
                .get("password")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (
                Protocol::Tuic,
                ProtocolSettings::Tuic {
                    uuid,
                    password,
                    congestion_control: v
                        .get("congestion-controller")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                },
            )
        }
        _ => return None,
    };

    let transport = match v.get("network").and_then(|x| x.as_str()) {
        Some("ws") => {
            let opts = v.get("ws-opts");
            Transport::Ws {
                path: opts
                    .and_then(|o| o.get("path"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("/")
                    .to_string(),
                host: opts
                    .and_then(|o| o.get("headers"))
                    .and_then(|h| h.get("Host"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                headers: vec![],
            }
        }
        Some("grpc") => Transport::Grpc {
            service_name: v
                .get("grpc-opts")
                .and_then(|o| o.get("grpc-service-name"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        },
        _ => Transport::Tcp,
    };

    let tls_enabled = v
        .get("tls")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
        || v.get("reality-opts").is_some();
    let server_name = v
        .get("servername")
        .or_else(|| v.get("sni"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let alpn = v
        .get("alpn")
        .and_then(|x| x.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let allow_insecure = v
        .get("skip-cert-verify")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let utls_fingerprint = v
        .get("client-fingerprint")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let reality = v.get("reality-opts").map(|r| RealitySettings {
        public_key: r
            .get("public-key")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        short_id: r
            .get("short-id")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        spider_x: None,
    });

    Some(ProxyProfile {
        id: ProxyProfile::new_id(),
        name,
        country_code: None,
        protocol,
        server,
        port,
        settings,
        transport,
        tls: TlsSettings {
            enabled: tls_enabled,
            server_name,
            alpn,
            allow_insecure,
            utls_fingerprint,
            reality,
        },
        subscription_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clash_yaml() {
        let body = br#"
proxies:
  - name: "NL 1"
    type: vless
    server: example.com
    port: 443
    uuid: 550e8400-e29b-41d4-a716-446655440000
    flow: xtls-rprx-vision
    network: tcp
    tls: true
    servername: www.cloudflare.com
    client-fingerprint: chrome
    reality-opts:
      public-key: AAAA
      short-id: "12"
"#;
        let v = parse(body).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "NL 1");
        assert!(v[0].tls.reality.is_some());
    }
}
