//! Best-effort removal of a stale Wintun adapter before a TUN connect.
//!
//! Why this exists: when the sing-box sidecar dies hard (BSOD, hard
//! taskkill, OOM), the kernel sometimes holds onto its Wintun virtual
//! adapter for longer than our usual 800 ms grace period. The next
//! connect tries to create the same adapter (`v2pn-tun`) and trips one
//! of:
//!   * `Cannot create a file when that file already exists.`
//!   * `Element not found.`
//!   * `The system cannot find the file specified.`
//!
//! Killing the kernel object directly requires the Wintun DLL handle that
//! the previous sing-box owned — which we obviously don't have any
//! more. The next-best option is to ask Windows to tear down the
//! interface metadata via `netsh`, which clears the IP configuration and
//! DNS routing tied to the adapter name, letting the kernel finalise its
//! cleanup. Combined with our existing 800 ms wintun grace sleep, this
//! turns a hard reconnect after a crash from "user has to manually open
//! Network Connections" into "just works".
//!
//! Failures here are non-fatal — the `netsh` invocation succeeds with
//! exit code 1 if there's no adapter to delete (which is the common
//! case), and even if it succeeds with 0 we don't really care because
//! the supervisor's main start path also has its own grace logic.

use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Run `netsh interface delete interface <name>` with output suppressed.
/// Returns instantly if `netsh` isn't on PATH (which would be a very
/// broken Windows install). Best-effort — never fails the call site.
pub fn cleanup_stale_adapter(adapter_name: &str) {
    if adapter_name.is_empty() {
        return;
    }

    #[cfg(windows)]
    {
        // Only allow alphanumerics + dashes / underscores. Anything else
        // would let an attacker who somehow controlled the adapter name
        // smuggle extra arguments into netsh. Our supervisor pins the
        // name to "v2pn-tun" but a future config option might let it be
        // user-configurable, so be defensive now.
        if !adapter_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            tracing::warn!(target: "wintun_cleanup",
                "refusing to clean up adapter with non-trivial name: {adapter_name:?}");
            return;
        }

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // First: drop any IPv4/IPv6 configuration tied to the adapter.
        // This is a no-op when the adapter is already gone.
        for family in &["ipv4", "ipv6"] {
            let _ = std::process::Command::new("netsh")
                .args(&["interface", family, "delete", "address", adapter_name, "all"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
        }

        // Second: try to delete the interface itself. Fails harmlessly
        // when there's nothing to delete; succeeds when there is.
        let result = std::process::Command::new("netsh")
            .args(&["interface", "delete", "interface", adapter_name])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        match result {
            Ok(o) if o.status.success() => {
                tracing::info!(target: "wintun_cleanup",
                    "removed stale adapter '{adapter_name}'");
                // Hold for the kernel to finalise the teardown — this
                // mirrors the post-stop grace in supervisor::stop. The
                // sing-box process is not yet running here so the sleep
                // is on the connect path, not on a hot loop.
                std::thread::sleep(Duration::from_millis(800));
            }
            _ => {
                // No adapter present, or we don't have rights. Either
                // way, nothing to do — the connect path handles its own
                // race-protection downstream.
            }
        }
    }

    #[cfg(not(windows))]
    {
        // POSIX TUN cleanup is handled by sing-box itself on init; we
        // don't ship the wintun-only adapter management story here.
        let _ = adapter_name;
    }
}
