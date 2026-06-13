//! Generate the actual config we'd ship to sing-box from a representative
//! profile and run the bundled binary's `check` subcommand on it. This is
//! the fastest way to surface schema regressions when sing-box minor
//! versions change their config validation.

use std::path::PathBuf;
use std::process::Command;

use app_core::profile::{
    Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings, Transport,
};
use app_core::singbox::config::{build_config, ConnectionMode, ConnectionOptions};
use app_core::singbox::sanitize::sanitize_strict;
use app_core::supervisor::resolve_singbox_binary;

fn locate_bin() -> Option<PathBuf> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let dir = PathBuf::from(manifest)
        .ancestors()
        .nth(2)?
        .join("crates/tauri-app/binaries");
    resolve_singbox_binary(&dir)
}

fn vless_buzz_like() -> ProxyProfile {
    ProxyProfile {
        id: ProxyProfile::new_id(),
        name: "USA".into(),
        country_code: None,
        protocol: Protocol::Vless,
        server: "1.2.3.4".into(),
        port: 443,
        settings: ProtocolSettings::Vless {
            uuid: "550e8400-e29b-41d4-a716-446655440000".into(),
            flow: Some("xtls-rprx-vision".into()),
        },
        transport: Transport::Tcp,
        tls: TlsSettings {
            enabled: true,
            server_name: Some("www.microsoft.com".into()),
            alpn: vec![],
            allow_insecure: false,
            utls_fingerprint: Some("chrome".into()),
            reality: Some(RealitySettings {
                public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
                short_id: Some("12".into()),
                spider_x: None,
            }),
        },
        subscription_id: None,
    }
}

#[test]
fn config_passes_singbox_check() {
    let Some(bin) = locate_bin() else {
        eprintln!("skipped: sing-box binary not present");
        return;
    };

    let opts = ConnectionOptions {
        mode: ConnectionMode::Proxy,
        mixed_port: 27890,
        clash_api_port: 29090,
        ..ConnectionOptions::default()
    };
    let mut cfg = build_config(&vless_buzz_like(), &opts);
    sanitize_strict(&mut cfg).expect("sanitiser");

    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("test.json");
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();

    let out = Command::new(&bin)
        .arg("check")
        .arg("-c")
        .arg(&cfg_path)
        .output()
        .expect("spawn sing-box check");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    if !out.status.success() {
        eprintln!("--- generated config ---");
        eprintln!("{}", String::from_utf8_lossy(&std::fs::read(&cfg_path).unwrap()));
        eprintln!("--- sing-box stderr ---");
        eprintln!("{stderr}");
        eprintln!("--- sing-box stdout ---");
        eprintln!("{stdout}");
        panic!(
            "sing-box check rejected our config: {}",
            stderr.lines().next().unwrap_or("(empty stderr)")
        );
    }
}

#[test]
fn config_passes_singbox_check_tun() {
    let Some(bin) = locate_bin() else {
        return;
    };

    let opts = ConnectionOptions {
        mode: ConnectionMode::Tun,
        mixed_port: 27891,
        clash_api_port: 29091,
        ..ConnectionOptions::default()
    };
    let mut cfg = build_config(&vless_buzz_like(), &opts);
    sanitize_strict(&mut cfg).expect("sanitiser");

    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("test.json");
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();

    let out = Command::new(&bin)
        .arg("check")
        .arg("-c")
        .arg(&cfg_path)
        .output()
        .expect("spawn");

    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        eprintln!("--- generated tun config ---");
        eprintln!("{}", String::from_utf8_lossy(&std::fs::read(&cfg_path).unwrap()));
        panic!("tun config rejected: {stderr}");
    }
}
