//! Internal proxy profile model.
//!
//! Designed as a *protocol-neutral* representation that we can compile down to
//! sing-box, Xray or any other engine. All sensitive fields (UUIDs, passwords,
//! private keys) are stored as plain strings here — encryption happens at the
//! `vault` layer when persisting.

use serde::{Deserialize, Serialize};

/// One proxy server (a single outbound).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyProfile {
    /// Stable client-generated id (uuid v4).
    pub id: String,
    /// Display name as seen in the subscription (e.g. `🇳🇱 LTE 2 | Обход белых списков`).
    pub name: String,
    /// Optional emoji or ISO country code parsed from the name (`NL`, `RU`, …).
    pub country_code: Option<String>,
    /// Wire-level protocol.
    pub protocol: Protocol,
    /// Server hostname or IP.
    pub server: String,
    /// Server port.
    pub port: u16,
    /// Protocol-specific settings.
    pub settings: ProtocolSettings,
    /// Transport layer (TCP / WS / gRPC / HTTPUpgrade / xHTTP).
    pub transport: Transport,
    /// TLS / REALITY layer.
    pub tls: TlsSettings,
    /// Optional tag of the source subscription.
    pub subscription_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
    Hysteria2,
    Tuic,
    AnyTls,
    Wireguard,
    Ssh,
    Socks,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProtocolSettings {
    Vless { uuid: String, flow: Option<String> },
    Vmess { uuid: String, alter_id: u32, security: String },
    Trojan { password: String },
    Shadowsocks { method: String, password: String },
    Hysteria2 { password: String, obfs: Option<Hysteria2Obfs> },
    Tuic { uuid: String, password: String, congestion_control: Option<String> },
    AnyTls { password: String },
    Wireguard {
        private_key: String,
        peer_public_key: String,
        pre_shared_key: Option<String>,
        local_address: Vec<String>,
        mtu: Option<u16>,
    },
    Ssh { user: String, password: Option<String>, private_key: Option<String> },
    Socks { user: Option<String>, password: Option<String> },
    Http { user: Option<String>, password: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub kind: String, // "salamander"
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Tcp,
    Ws { path: String, host: Option<String>, headers: Vec<(String, String)> },
    Grpc { service_name: String },
    HttpUpgrade { path: String, host: Option<String> },
    XHttp { path: String, mode: Option<String> },
    Quic,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsSettings {
    pub enabled: bool,
    pub server_name: Option<String>,
    pub alpn: Vec<String>,
    pub allow_insecure: bool,
    pub utls_fingerprint: Option<String>, // chrome, firefox, safari, randomized…
    pub reality: Option<RealitySettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealitySettings {
    pub public_key: String,
    pub short_id: Option<String>,
    pub spider_x: Option<String>,
}

impl ProxyProfile {
    pub fn new_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}
