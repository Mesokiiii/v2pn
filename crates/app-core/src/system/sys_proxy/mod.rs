//! System proxy management (per-OS).
//!
//! Goals — bullet-proof, never leave the user without internet:
//!
//! 1. **Snapshot before mutate.** Capture the user's existing settings
//!    (`ProxyEnable`, `ProxyServer`, `ProxyOverride`, `AutoConfigURL`)
//!    *before* writing anything, so we can always restore them.
//! 2. **Atomic apply.** Update via Windows registry under
//!    `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`,
//!    then notify Wininet via `InternetSetOption` so running browsers/apps
//!    pick up the change immediately (no restart required).
//! 3. **Restore must be idempotent and infallible.** Even after BSOD or
//!    suspend-storm, `restore()` reverts back to the captured snapshot.
//!
//! On non-Windows hosts the implementation is a no-op stub so `app-core`
//! still builds.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::WindowsSystemProxy as ActiveSystemProxy;

#[cfg(not(windows))]
pub mod stub;
#[cfg(not(windows))]
pub use stub::StubSystemProxy as ActiveSystemProxy;

/// Snapshot of OS proxy settings — used both as "what we replaced" and
/// "what to restore on shutdown".
///
/// Stored in the on-disk state file so a crash on this run can be cleaned up
/// by the next launch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxySnapshot {
    pub proxy_enable: u32,
    pub proxy_server: Option<String>,
    pub proxy_override: Option<String>,
    pub auto_config_url: Option<String>,
}

impl ProxySnapshot {
    pub fn was_enabled(&self) -> bool {
        self.proxy_enable != 0
    }
}

/// What to call the OS proxy abstraction by. Kept as a trait so power-event
/// handlers and the recovery code can manipulate it without caring whether
/// we're on Windows, macOS or Linux.
pub trait SystemProxy: Send + Sync {
    fn snapshot(&self) -> crate::CoreResult<ProxySnapshot>;
    /// Apply a proxy server "host:port" and override list (semicolon separated).
    fn apply(&self, addr: &str, bypass: &[&str]) -> crate::CoreResult<()>;
    /// Restore exactly the snapshot. Should be safe to call multiple times.
    fn restore(&self, snap: &ProxySnapshot) -> crate::CoreResult<()>;
}
