//! Stable per-machine identifier used for the `X-Hwid` request header.
//!
//! Many Remnawave-based panels (BuzzVPN being the canonical example) gate
//! the subscription endpoint behind this header — without it they reply with
//! a 302 to the webapp installer; with *any* stable value they return the
//! real subscription body.
//!
//! ## Stability matters
//! The HWID must be **deterministic per host**. If we sent a random value on
//! each request the panel would count us as a new device every time and
//! eventually trip its device-limit guardrail.
//!
//! ## Privacy
//! The value never leaves this machine in plain form. We hash the source
//! identity (Windows MachineGuid / hostname) with SHA-256 and only ship a
//! 32-char hex prefix. The panel learns nothing about the user beyond
//! "this is the same install as last time".

use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};

static CACHED: Lazy<String> = Lazy::new(compute);

/// Returns the stable HWID. Cached for the process lifetime.
pub fn hwid() -> &'static str {
    CACHED.as_str()
}

fn compute() -> String {
    let raw = collect_source().unwrap_or_else(|| "v2pn-fallback-id".to_string());
    let mut h = Sha256::new();
    h.update(b"v2pn-hwid-v1\0");
    h.update(raw.as_bytes());
    let digest = h.finalize();
    // 16 bytes / 32 hex chars — plenty of identity, looks like a UUID.
    hex::encode(&digest[..16])
}

#[cfg(windows)]
fn collect_source() -> Option<String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;

    // Windows MachineGuid lives in the 64-bit view of the registry — the
    // KEY_WOW64_64KEY flag is required when running as a 32-bit process.
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .ok()?;
    let guid: String = key.get_value("MachineGuid").ok()?;
    Some(guid)
}

#[cfg(not(windows))]
fn collect_source() -> Option<String> {
    // /etc/machine-id on Linux, /Library/Preferences/SystemConfiguration/com.apple.smb.server.plist
    // would be the equivalents — left as a TODO when we ship those targets.
    std::env::var("HOSTNAME").ok().or_else(|| std::env::var("USER").ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hwid_is_stable_within_process() {
        let a = hwid();
        let b = hwid();
        assert_eq!(a, b);
        assert_eq!(a.len(), 32, "expected 16-byte hex hwid");
    }
}
