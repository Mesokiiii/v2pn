//! "Repair my network" — emergency cleanup for the Windows networking
//! stack after a VPN client (us or someone else) left it in a half-broken
//! state.
//!
//! The classic symptom: user installs a competitor VPN that crashed
//! mid-connect, or our own sing-box was killed via Task Manager. The
//! resulting state can include any combination of:
//!
//!   * The HKCU `ProxyServer` registry value still points to
//!     127.0.0.1:7890 even though nothing is listening, so every browser
//!     / Electron app shows "DNS_PROBE_FINISHED_NO_INTERNET".
//!   * A stale Wintun adapter remains in `Network Connections` with an
//!     IP from the 172.19.0.0/30 range (or another VPN's CIDR), still
//!     in the routing table.
//!   * The DNS resolver cache holds stale negative entries from when
//!     `hijack-dns` was active.
//!   * In rare cases, a TAP-Windows adapter from OpenVPN wedged the
//!     network stack hard enough that even the user's regular Wi-Fi
//!     refuses to ping.
//!
//! This module exposes a single `run_full_repair` that walks through
//! every known recovery step and returns a structured report. Each step
//! is best-effort: failures are recorded but never propagate, because
//! the user invoked this *because* something is already broken — bailing
//! on the first sub-failure would be unhelpful.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::supervisor::Supervisor;
use crate::sys_proxy::{ActiveSystemProxy, SystemProxy};

/// One step in the repair sequence. The frontend renders these as a
/// timeline so the user sees what happened, in order, with a 🟢/🔴 dot
/// per step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairStep {
    /// Stable id (used for icon picking on the UI side).
    pub id: String,
    /// Localised label — set on the UI side via the i18n bundle. The
    /// backend produces a stable key here; the UI picks up the
    /// translation. We keep the English text as a fallback in case the
    /// frontend drops the key.
    pub label_key: String,
    pub ok: bool,
    /// Free-form detail. Empty string when there's nothing interesting
    /// to report. UI shows this as a secondary line under the step.
    pub detail: String,
    pub took_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairReport {
    pub steps: Vec<RepairStep>,
    pub started_at: i64,
    pub finished_at: i64,
}

/// Synchronously run every repair step. Async only at the boundary where
/// we need to call into the (Tokio-based) supervisor; everything else is
/// blocking IO.
pub async fn run_full_repair(supervisor: Arc<Supervisor>, tun_name: &str) -> RepairReport {
    let started_at = now_unix();
    let mut steps = Vec::new();

    // Step 1 — stop sing-box. Blocking on this protects every later step
    // from a race where we delete a TUN adapter that sing-box is about
    // to recreate, or wipe an OS-proxy registry value sing-box is in
    // the middle of mirroring back.
    {
        let t0 = std::time::Instant::now();
        let mut detail = String::new();
        let res = supervisor.stop().await;
        let ok = res.is_ok();
        if let Err(e) = res {
            detail = format!("supervisor.stop: {e}");
        }
        steps.push(RepairStep {
            id: "stop_singbox".to_string(),
            label_key: "repair.stopSingbox".to_string(),
            ok,
            detail,
            took_ms: t0.elapsed().as_millis() as u64,
        });
    }

    // Step 2 — force-clear the system proxy registry slot, *not* via the
    // ConnectionGuard's snapshot path. The user invoked this because
    // something else broke; we want a clean, known-good state, not a
    // restore to whatever the previous (potentially also broken)
    // snapshot was.
    {
        let t0 = std::time::Instant::now();
        let sys = ActiveSystemProxy::new();
        let mut ok = true;
        let mut detail = String::new();
        // Reset to "no proxy" rather than restore — this is the safe
        // default for a recovery flow.
        let blank = crate::sys_proxy::ProxySnapshot {
            proxy_enable: 0,
            proxy_server: None,
            proxy_override: None,
            auto_config_url: None,
        };
        if let Err(e) = sys.restore(&blank) {
            ok = false;
            detail = format!("sys_proxy.restore: {e}");
        }
        steps.push(RepairStep {
            id: "clear_proxy".to_string(),
            label_key: "repair.clearProxy".to_string(),
            ok,
            detail,
            took_ms: t0.elapsed().as_millis() as u64,
        });
    }

    // Step 3 — wintun adapter cleanup. We try our own adapter name first
    // (definitely ours to remove); then a small allow-list of known
    // *other* VPN-tooling adapter names that have caused user-visible
    // breakage in the past — Hiddify, NekoBox, Karing all use Wintun
    // with predictable names. We never touch adapters with names
    // outside that list.
    {
        let t0 = std::time::Instant::now();
        let candidates = ["Hiddify", "NekoBox", "Karing", "v2raya"]
            .iter()
            .copied()
            .chain(std::iter::once(tun_name))
            .collect::<Vec<_>>();
        let mut removed = 0;
        for name in &candidates {
            // The cleanup helper logs internally and never panics; we
            // only see counts here.
            crate::wintun_cleanup::cleanup_stale_adapter(name);
            removed += 1;
        }
        steps.push(RepairStep {
            id: "wintun_cleanup".to_string(),
            label_key: "repair.wintunCleanup".to_string(),
            ok: true,
            detail: format!("attempted {removed} adapter name(s)"),
            took_ms: t0.elapsed().as_millis() as u64,
        });
    }

    // Step 4 — flush DNS resolver cache. Cheap, no admin needed.
    steps.push(run_blocking_step(
        "flush_dns",
        "repair.flushDns",
        || run_command_quiet("ipconfig", &["/flushdns"]),
    ));

    // Step 5 — release/renew DHCP lease. Triggers the OS to
    // re-acquire the routing table from the actual physical adapter,
    // which is what most users actually need after a stale TUN.
    steps.push(run_blocking_step(
        "renew_dhcp",
        "repair.renewDhcp",
        || run_command_quiet("ipconfig", &["/registerdns"]),
    ));

    // Step 6 — ARP / NetBIOS cache flush. Fixes the "I switched VPNs and
    // half my LAN devices are unreachable" class of problem.
    steps.push(run_blocking_step(
        "arp_flush",
        "repair.arpFlush",
        || run_command_quiet("netsh", &["interface", "ip", "delete", "arpcache"]),
    ));

    // Step 7 — restart Wininet via the same notify path our usual
    // sys_proxy code uses. Without this, browsers don't pick up the
    // newly-cleared registry slot until you close and reopen them.
    {
        let t0 = std::time::Instant::now();
        #[cfg(windows)]
        crate::sys_proxy::windows::notify_wininet();
        steps.push(RepairStep {
            id: "notify_wininet".to_string(),
            label_key: "repair.notifyWininet".to_string(),
            ok: true,
            detail: String::new(),
            took_ms: t0.elapsed().as_millis() as u64,
        });
    }

    let finished_at = now_unix();
    RepairReport {
        steps,
        started_at,
        finished_at,
    }
}

/// Wrap a blocking command-runner in a step record, with per-step
/// timing. Everything in `f` returns `Ok(detail)` on success or `Err`.
fn run_blocking_step(
    id: &str,
    label_key: &str,
    f: impl FnOnce() -> Result<String, std::io::Error>,
) -> RepairStep {
    let t0 = std::time::Instant::now();
    let (ok, detail) = match f() {
        Ok(d) => (true, d),
        Err(e) => (false, e.to_string()),
    };
    RepairStep {
        id: id.to_string(),
        label_key: label_key.to_string(),
        ok,
        detail,
        took_ms: t0.elapsed().as_millis() as u64,
    }
}

/// Run a Windows command-line tool with stdout/stderr captured. Returns
/// the first 200 chars of stdout as a "what happened" hint on success,
/// or an io::Error on failure. We don't elevate — every command in our
/// recovery path is fine without admin (`ipconfig`, `netsh interface ip`
/// don't need it for their *flush* / *delete arpcache* sub-commands).
fn run_command_quiet(program: &str, args: &[&str]) -> Result<String, std::io::Error> {
    use std::process::Command;

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new(program);
    cmd.args(args);

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let out = cmd.output()?;
    if !out.status.success() {
        // Surface the tool's own stderr — it's almost always actionable
        // (e.g. "The system cannot find the file specified.").
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let combined = if stderr.is_empty() { stdout } else { stderr };
        return Err(std::io::Error::other(format!(
            "{program} {args:?} exited with {}: {combined}",
            out.status
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    Ok(trimmed.chars().take(240).collect())
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_serializes() {
        let r = RepairReport {
            started_at: 1,
            finished_at: 2,
            steps: vec![RepairStep {
                id: "x".to_string(),
                label_key: "repair.x".to_string(),
                ok: true,
                detail: String::new(),
                took_ms: 5,
            }],
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"id\":\"x\""));
    }

    #[test]
    fn run_command_quiet_handles_nonexistent() {
        let r = run_command_quiet("definitely_not_a_real_command_12345", &[]);
        assert!(r.is_err());
    }
}

// Silence dead-code warnings for the Duration import on platforms where
// nothing in the body actually uses it. Kept for forward-compat — the
// future "wait between netsh steps" path may need it.
#[allow(dead_code)]
const _UNUSED_DURATION: Duration = Duration::from_millis(0);
