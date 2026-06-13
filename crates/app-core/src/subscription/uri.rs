//! Share-link URI parsers (`vless://`, `vmess://`, `trojan://`, `ss://`, `hy2://`, `tuic://`).

use std::collections::HashMap;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::error::{CoreError, CoreResult};
use crate::profile::{
    Hysteria2Obfs, Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings,
    Transport,
};
use crate::subscription::base64::decode_loose;

/// Parse one share URI into a profile, or return [`CoreError::UnsupportedScheme`].
pub fn parse_uri(input: &str) -> CoreResult<ProxyProfile> {
    let input = input.trim();
    let scheme = input
        .split_once("://")
        .map(|(s, _)| s.to_ascii_lowercase())
        .unwrap_or_default();

    match scheme.as_str() {
        "vless" => parse_vless(input),
        "vmess" => parse_vmess(input),
        "trojan" => parse_trojan(input),
        "ss" => parse_shadowsocks(input),
        "hy2" | "hysteria2" => parse_hysteria2(input),
        "tuic" => parse_tuic(input),
        other => Err(CoreError::UnsupportedScheme(other.to_string())),
    }
}

fn fragment_name(url: &Url, fallback: &str) -> String {
    url.fragment()
        .map(|f| {
            percent_decode_str(f)
                .decode_utf8_lossy()
                .into_owned()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn query_map(url: &Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned().to_ascii_lowercase(), v.into_owned()))
        .collect()
}

fn require_host(url: &Url) -> CoreResult<String> {
    url.host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| CoreError::Parse("missing host".into()))
}

fn require_port(url: &Url) -> CoreResult<u16> {
    url.port()
        .ok_or_else(|| CoreError::Parse("missing port".into()))
}

fn build_transport(q: &HashMap<String, String>) -> Transport {
    let net = q
        .get("type")
        .or_else(|| q.get("network"))
        .map(String::as_str)
        .unwrap_or("tcp");
    match net {
        "ws" => Transport::Ws {
            path: q.get("path").cloned().unwrap_or_else(|| "/".to_string()),
            host: q.get("host").cloned(),
            headers: Vec::new(),
        },
        "grpc" => Transport::Grpc {
            service_name: q.get("servicename").cloned().unwrap_or_default(),
        },
        "httpupgrade" => Transport::HttpUpgrade {
            path: q.get("path").cloned().unwrap_or_else(|| "/".to_string()),
            host: q.get("host").cloned(),
        },
        "xhttp" | "splithttp" => Transport::XHttp {
            path: q.get("path").cloned().unwrap_or_else(|| "/".to_string()),
            mode: q.get("mode").cloned(),
        },
        "quic" => Transport::Quic,
        // tcp / raw / http
        _ => Transport::Tcp,
    }
}

fn build_tls(q: &HashMap<String, String>) -> TlsSettings {
    let security = q.get("security").map(String::as_str).unwrap_or("none");
    let enabled = matches!(security, "tls" | "reality" | "xtls");
    let reality = if security == "reality" {
        Some(RealitySettings {
            public_key: q.get("pbk").cloned().unwrap_or_default(),
            short_id: q.get("sid").cloned(),
            spider_x: q.get("spx").cloned(),
        })
    } else {
        None
    };
    let alpn = q
        .get("alpn")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_default();
    TlsSettings {
        enabled,
        server_name: q.get("sni").cloned().or_else(|| q.get("peer").cloned()),
        alpn,
        allow_insecure: matches!(
            q.get("allowinsecure").map(String::as_str),
            Some("1") | Some("true")
        ),
        utls_fingerprint: q.get("fp").cloned(),
        reality,
    }
}

fn parse_vless(input: &str) -> CoreResult<ProxyProfile> {
    let url = Url::parse(input)?;
    let uuid = url.username().to_string();
    if uuid.is_empty() {
        return Err(CoreError::Parse("vless: missing uuid".into()));
    }
    let server = require_host(&url)?;
    let port = require_port(&url)?;
    let q = query_map(&url);

    let flow = q.get("flow").cloned().filter(|s| !s.is_empty());
    let transport = build_transport(&q);
    let tls = build_tls(&q);

    Ok(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: fragment_name(&url, &server),
        country_code: None,
        protocol: Protocol::Vless,
        server,
        port,
        settings: ProtocolSettings::Vless { uuid, flow },
        transport,
        tls,
        subscription_id: None,
    })
}

fn parse_vmess(input: &str) -> CoreResult<ProxyProfile> {
    // vmess:// uses base64-of-JSON
    let payload = input.trim_start_matches("vmess://");
    let decoded = decode_loose(payload)?;
    let json: serde_json::Value = serde_json::from_slice(&decoded)
        .map_err(|e| CoreError::Parse(format!("vmess json: {e}")))?;

    let server = json["add"].as_str().ok_or_else(|| CoreError::Parse("vmess: add".into()))?;
    let port = json["port"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| json["port"].as_u64().map(|p| p as u16))
        .ok_or_else(|| CoreError::Parse("vmess: port".into()))?;
    let uuid = json["id"].as_str().ok_or_else(|| CoreError::Parse("vmess: id".into()))?;
    let alter_id = json["aid"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| json["aid"].as_u64().map(|x| x as u32))
        .unwrap_or(0);
    let security = json["scy"].as_str().unwrap_or("auto").to_string();
    let net = json["net"].as_str().unwrap_or("tcp");
    let path = json["path"].as_str().unwrap_or("/").to_string();
    let host = json["host"].as_str().map(str::to_string);
    let name = json["ps"].as_str().unwrap_or(server).to_string();

    let transport = match net {
        "ws" => Transport::Ws { path, host, headers: vec![] },
        "grpc" => Transport::Grpc {
            service_name: json["path"].as_str().unwrap_or("").to_string(),
        },
        _ => Transport::Tcp,
    };

    let tls_enabled = matches!(json["tls"].as_str(), Some("tls") | Some("reality"));
    let tls = TlsSettings {
        enabled: tls_enabled,
        server_name: json["sni"].as_str().map(str::to_string),
        alpn: vec![],
        allow_insecure: false,
        utls_fingerprint: json["fp"].as_str().map(str::to_string),
        reality: None,
    };

    Ok(ProxyProfile {
        id: ProxyProfile::new_id(),
        name,
        country_code: None,
        protocol: Protocol::Vmess,
        server: server.to_string(),
        port,
        settings: ProtocolSettings::Vmess { uuid: uuid.to_string(), alter_id, security },
        transport,
        tls,
        subscription_id: None,
    })
}

fn parse_trojan(input: &str) -> CoreResult<ProxyProfile> {
    let url = Url::parse(input)?;
    let password = percent_decode_str(url.username()).decode_utf8_lossy().into_owned();
    let server = require_host(&url)?;
    let port = require_port(&url)?;
    let q = query_map(&url);

    Ok(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: fragment_name(&url, &server),
        country_code: None,
        protocol: Protocol::Trojan,
        server,
        port,
        settings: ProtocolSettings::Trojan { password },
        transport: build_transport(&q),
        tls: TlsSettings {
            enabled: !matches!(q.get("security").map(String::as_str), Some("none")),
            ..build_tls(&q)
        },
        subscription_id: None,
    })
}

fn parse_shadowsocks(input: &str) -> CoreResult<ProxyProfile> {
    // Two flavours:
    //   ss://method:password@host:port#name              (SIP002)
    //   ss://BASE64(method:password)@host:port#name      (legacy)
    //   ss://BASE64(method:password@host:port)#name      (very old)
    let after = input.trim_start_matches("ss://");
    let (rest, fragment) = match after.split_once('#') {
        Some((r, f)) => (r, percent_decode_str(f).decode_utf8_lossy().into_owned()),
        None => (after, String::new()),
    };

    if let Some((userinfo, hostpart)) = rest.split_once('@') {
        // Try plain (SIP002)
        let userinfo_dec = percent_decode_str(userinfo).decode_utf8_lossy().into_owned();
        let userinfo_decoded = if userinfo_dec.contains(':') {
            userinfo_dec
        } else if let Ok(b) = decode_loose(&userinfo_dec) {
            String::from_utf8_lossy(&b).into_owned()
        } else {
            userinfo_dec
        };
        let (method, password) = userinfo_decoded
            .split_once(':')
            .ok_or_else(|| CoreError::Parse("ss: userinfo".into()))?;

        let hostpart_q = hostpart.split_once('?').map(|(h, _)| h).unwrap_or(hostpart);
        let (host, port) = hostpart_q
            .rsplit_once(':')
            .ok_or_else(|| CoreError::Parse("ss: host:port".into()))?;
        let port: u16 = port.parse().map_err(|_| CoreError::Parse("ss: port".into()))?;

        return Ok(ProxyProfile {
            id: ProxyProfile::new_id(),
            name: if fragment.is_empty() { host.to_string() } else { fragment },
            country_code: None,
            protocol: Protocol::Shadowsocks,
            server: host.to_string(),
            port,
            settings: ProtocolSettings::Shadowsocks {
                method: method.to_string(),
                password: password.to_string(),
            },
            transport: Transport::Tcp,
            tls: TlsSettings::default(),
            subscription_id: None,
        });
    }

    // Legacy: whole rest is base64
    let decoded = decode_loose(rest)?;
    let s = std::str::from_utf8(&decoded)?;
    let (cred, hostport) = s.split_once('@').ok_or_else(|| CoreError::Parse("ss legacy".into()))?;
    let (method, password) = cred.split_once(':').ok_or_else(|| CoreError::Parse("ss legacy cred".into()))?;
    let (host, port) = hostport.rsplit_once(':').ok_or_else(|| CoreError::Parse("ss legacy host".into()))?;
    let port: u16 = port.parse().map_err(|_| CoreError::Parse("ss: port".into()))?;
    Ok(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: if fragment.is_empty() { host.to_string() } else { fragment },
        country_code: None,
        protocol: Protocol::Shadowsocks,
        server: host.to_string(),
        port,
        settings: ProtocolSettings::Shadowsocks {
            method: method.to_string(),
            password: password.to_string(),
        },
        transport: Transport::Tcp,
        tls: TlsSettings::default(),
        subscription_id: None,
    })
}

fn parse_hysteria2(input: &str) -> CoreResult<ProxyProfile> {
    let url = Url::parse(input)?;
    let password = percent_decode_str(url.username()).decode_utf8_lossy().into_owned();
    let server = require_host(&url)?;
    let port = require_port(&url)?;
    let q = query_map(&url);

    let obfs = q.get("obfs").and_then(|kind| {
        q.get("obfs-password").map(|p| Hysteria2Obfs {
            kind: kind.clone(),
            password: p.clone(),
        })
    });

    Ok(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: fragment_name(&url, &server),
        country_code: None,
        protocol: Protocol::Hysteria2,
        server,
        port,
        settings: ProtocolSettings::Hysteria2 { password, obfs },
        transport: Transport::Quic,
        tls: TlsSettings {
            enabled: true,
            server_name: q.get("sni").cloned(),
            alpn: q
                .get("alpn")
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default(),
            allow_insecure: matches!(q.get("insecure").map(String::as_str), Some("1") | Some("true")),
            utls_fingerprint: q.get("fp").cloned(),
            reality: None,
        },
        subscription_id: None,
    })
}

fn parse_tuic(input: &str) -> CoreResult<ProxyProfile> {
    let url = Url::parse(input)?;
    let user = url.username();
    let password = percent_decode_str(url.password().unwrap_or(""))
        .decode_utf8_lossy()
        .into_owned();
    let server = require_host(&url)?;
    let port = require_port(&url)?;
    let q = query_map(&url);

    Ok(ProxyProfile {
        id: ProxyProfile::new_id(),
        name: fragment_name(&url, &server),
        country_code: None,
        protocol: Protocol::Tuic,
        server,
        port,
        settings: ProtocolSettings::Tuic {
            uuid: user.to_string(),
            password,
            congestion_control: q.get("congestion_control").cloned(),
        },
        transport: Transport::Quic,
        tls: TlsSettings {
            enabled: true,
            server_name: q.get("sni").cloned(),
            alpn: q
                .get("alpn")
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default(),
            allow_insecure: matches!(q.get("allow_insecure").map(String::as_str), Some("1") | Some("true")),
            utls_fingerprint: None,
            reality: None,
        },
        subscription_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vless_reality_vision() {
        let uri = "vless://550e8400-e29b-41d4-a716-446655440000@example.com:443?type=tcp&security=reality&pbk=AAAA&fp=chrome&sni=www.cloudflare.com&sid=1234&spx=%2F&flow=xtls-rprx-vision#NL%201";
        let p = parse_uri(uri).unwrap();
        assert_eq!(p.name, "NL 1");
        assert_eq!(p.server, "example.com");
        assert_eq!(p.port, 443);
        assert!(matches!(p.protocol, Protocol::Vless));
        match p.settings {
            ProtocolSettings::Vless { uuid, flow } => {
                assert_eq!(uuid, "550e8400-e29b-41d4-a716-446655440000");
                assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
            }
            _ => panic!("wrong settings"),
        }
        let r = p.tls.reality.unwrap();
        assert_eq!(r.public_key, "AAAA");
        assert_eq!(r.short_id.as_deref(), Some("1234"));
        assert_eq!(p.tls.utls_fingerprint.as_deref(), Some("chrome"));
        assert_eq!(p.tls.server_name.as_deref(), Some("www.cloudflare.com"));
    }

    #[test]
    fn parses_trojan() {
        let uri = "trojan://my%20pass@srv.example:8443?sni=cdn.example#t1";
        let p = parse_uri(uri).unwrap();
        assert_eq!(p.name, "t1");
        match p.settings {
            ProtocolSettings::Trojan { password } => assert_eq!(password, "my pass"),
            _ => panic!(),
        }
    }

    #[test]
    fn parses_ss_sip002() {
        let uri = "ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ=@1.2.3.4:8388#node";
        let p = parse_uri(uri).unwrap();
        match p.settings {
            ProtocolSettings::Shadowsocks { method, password } => {
                assert_eq!(method, "aes-256-gcm");
                assert_eq!(password, "password");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_hysteria2() {
        let uri = "hy2://strongpass@h2.example.com:443?sni=h2.example.com&alpn=h3#hy2-node";
        let p = parse_uri(uri).unwrap();
        assert_eq!(p.protocol, Protocol::Hysteria2);
        assert_eq!(p.port, 443);
    }
}
