//! End-to-end smoke test: spawn sing-box with a deliberately unreachable
//! server, verify the supervisor reports Failed/Idle and the ConnectionGuard
//! correctly restores the system proxy.
//!
//! Skipped when the bundled sing-box binary is missing (CI without
//! `scripts\fetch-singbox.ps1` run beforehand).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app_core::profile::{
    Protocol, ProtocolSettings, ProxyProfile, RealitySettings, TlsSettings, Transport,
};
use app_core::singbox::{
    config::{build_config, ConnectionMode, ConnectionOptions},
    sanitize::sanitize_strict,
};
use app_core::state_guard::ConnectionGuard;
use app_core::supervisor::{resolve_singbox_binary, ConnectionState, Supervisor};
use app_core::sys_proxy::{ActiveSystemProxy, SystemProxy};

/// Build a profile that will surely fail to connect (RFC 5737 TEST-NET).
fn unreachable_vless() -> ProxyProfile {
    ProxyProfile {
        id: ProxyProfile::new_id(),
        name: "smoke-test-target".into(),
        country_code: None,
        protocol: Protocol::Vless,
        server: "192.0.2.1".into(), // documented unroutable address
        port: 443,
        settings: ProtocolSettings::Vless {
            uuid: "00000000-0000-0000-0000-000000000000".into(),
            flow: Some("xtls-rprx-vision".into()),
        },
        transport: Transport::Tcp,
        tls: TlsSettings {
            enabled: true,
            server_name: Some("example.com".into()),
            alpn: vec![],
            allow_insecure: false,
            utls_fingerprint: Some("chrome".into()),
            reality: Some(RealitySettings {
                public_key: "AAAA".into(),
                short_id: Some("12".into()),
                spider_x: None,
            }),
        },
        subscription_id: None,
    }
}

fn locate_sing_box() -> Option<PathBuf> {
    // Walk up from CARGO_MANIFEST_DIR to the workspace root, then into binaries/.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let dir = PathBuf::from(manifest)
        .ancestors()
        .nth(2)?                                                  // <repo>
        .join("crates/tauri-app/binaries");
    resolve_singbox_binary(&dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_lifecycle_unreachable_server() {
    let Some(bin) = locate_sing_box() else {
        eprintln!("skipped: sing-box binary not present (run scripts\\fetch-singbox.ps1)");
        return;
    };
    let runtime = tempfile::tempdir().unwrap();
    let supervisor = Arc::new(Supervisor::new(bin, runtime.path().to_path_buf()).unwrap());
    let mut state_rx = supervisor.subscribe_state();

    let opts = ConnectionOptions {
        mode: ConnectionMode::Proxy,
        // Use ports unlikely to clash with any other instance.
        mixed_port: 27890,
        clash_api_port: 29090,
        ..ConnectionOptions::default()
    };
    let mut cfg = build_config(&unreachable_vless(), &opts);
    sanitize_strict(&mut cfg).expect("config passes sanitiser");

    supervisor
        .start(&cfg, opts.mode)
        .await
        .expect("sing-box spawned");

    // Wait either for Connected (sing-box came up but won't actually proxy
    // anything — that's fine for this lifecycle test) or for Failed.
    let mut got_terminal_state = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(1), state_rx.recv()).await {
            Ok(Ok(s)) => {
                eprintln!("smoke: state -> {s:?}");
                match s {
                    ConnectionState::Connected | ConnectionState::Failed { .. } => {
                        got_terminal_state = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ => continue,
        }
    }
    assert!(
        got_terminal_state,
        "supervisor never reached Connected or Failed within 8s"
    );

    supervisor.stop().await.expect("clean stop");
    assert!(
        matches!(supervisor.state(), ConnectionState::Idle),
        "expected Idle after stop, got {:?}", supervisor.state()
    );
}

#[test]
#[cfg(windows)]
fn guard_restores_proxy_on_drop() {
    let runtime = tempfile::tempdir().unwrap();
    let sys = ActiveSystemProxy::new();
    let before = sys.snapshot().expect("read current proxy");

    {
        let _g = ConnectionGuard::acquire_proxy(runtime.path(), "127.0.0.1:27890", &["localhost"])
            .expect("acquire");
        let mid = sys.snapshot().expect("snapshot mid");
        assert_eq!(mid.proxy_enable, 1, "proxy should be enabled after acquire");
        assert_eq!(mid.proxy_server.as_deref(), Some("127.0.0.1:27890"));
        // _g drops here
    }

    let after = sys.snapshot().expect("read final");
    assert_eq!(
        after, before,
        "Drop must restore the proxy snapshot exactly"
    );
}
