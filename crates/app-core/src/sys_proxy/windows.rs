//! Windows implementation of [`SystemProxy`].
//!
//! Settings live under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`.
//!
//! Crucially, we *always* notify Wininet via `InternetSetOption` after
//! writing — otherwise the registry change won't propagate to running apps
//! (Edge, Outlook, Steam, Visual Studio, every Electron app…) and the user
//! sees half-broken connectivity until the next reboot.

#![cfg(windows)]

use std::ffi::c_void;

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

use crate::error::CoreError;
use crate::sys_proxy::{ProxySnapshot, SystemProxy};
use crate::CoreResult;

const INTERNET_SETTINGS: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[derive(Debug, Default, Clone)]
pub struct WindowsSystemProxy;

impl WindowsSystemProxy {
    pub fn new() -> Self {
        Self
    }

    fn open_read(&self) -> CoreResult<RegKey> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(INTERNET_SETTINGS, KEY_READ)
            .map_err(|e| CoreError::Other(format!("open IE settings (read): {e}")))
    }

    fn open_write(&self) -> CoreResult<RegKey> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(INTERNET_SETTINGS, KEY_SET_VALUE | KEY_READ)
            .map_err(|e| CoreError::Other(format!("open IE settings (write): {e}")))
    }
}

impl SystemProxy for WindowsSystemProxy {
    fn snapshot(&self) -> CoreResult<ProxySnapshot> {
        let key = self.open_read()?;
        Ok(ProxySnapshot {
            proxy_enable: key.get_value("ProxyEnable").unwrap_or(0u32),
            proxy_server: key.get_value::<String, _>("ProxyServer").ok(),
            proxy_override: key.get_value::<String, _>("ProxyOverride").ok(),
            auto_config_url: key.get_value::<String, _>("AutoConfigURL").ok(),
        })
    }

    fn apply(&self, addr: &str, bypass: &[&str]) -> CoreResult<()> {
        let key = self.open_write()?;
        let bypass_str = bypass.join(";");

        key.set_value("ProxyEnable", &1u32)
            .map_err(|e| CoreError::Other(format!("set ProxyEnable: {e}")))?;
        key.set_value("ProxyServer", &addr.to_string())
            .map_err(|e| CoreError::Other(format!("set ProxyServer: {e}")))?;
        if !bypass.is_empty() {
            key.set_value("ProxyOverride", &bypass_str)
                .map_err(|e| CoreError::Other(format!("set ProxyOverride: {e}")))?;
        }
        // We do NOT touch AutoConfigURL — it's user's PAC; if they have one
        // we leave it (Windows priority: ProxyServer > AutoConfigURL when
        // ProxyEnable=1, but PAC would still load on top in some apps).

        notify_wininet();
        tracing::info!(target = "sys_proxy", "applied {addr}");
        Ok(())
    }

    fn restore(&self, snap: &ProxySnapshot) -> CoreResult<()> {
        let key = self.open_write()?;
        key.set_value("ProxyEnable", &snap.proxy_enable)
            .map_err(|e| CoreError::Other(format!("restore ProxyEnable: {e}")))?;

        match &snap.proxy_server {
            Some(s) => key
                .set_value("ProxyServer", s)
                .map_err(|e| CoreError::Other(format!("restore ProxyServer: {e}")))?,
            None => {
                let _ = key.delete_value("ProxyServer");
            }
        }
        match &snap.proxy_override {
            Some(s) => key
                .set_value("ProxyOverride", s)
                .map_err(|e| CoreError::Other(format!("restore ProxyOverride: {e}")))?,
            None => {
                let _ = key.delete_value("ProxyOverride");
            }
        }

        notify_wininet();
        tracing::info!(target = "sys_proxy", "restored to snapshot enabled={}", snap.proxy_enable);
        Ok(())
    }
}

/// Tell Wininet to re-read settings. Without this, browsers/Electron apps
/// continue using stale proxy info until they're restarted (or, often,
/// until reboot — exactly the bug the user complained about).
fn notify_wininet() {
    use ::windows::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_PROXY_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH,
    };

    unsafe {
        // 39 — INTERNET_OPTION_SETTINGS_CHANGED is what we'd love but is
        // documented as "obsolete" in modern docs. The combination below is
        // what every real-world client (Clash, sing-box-app, NekoRay) ships.
        let _ = InternetSetOptionW(
            None,
            INTERNET_OPTION_PROXY_SETTINGS_CHANGED,
            None,
            0,
        );
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
    let _ = std::ptr::null::<c_void>(); // silence unused-import if features change
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_does_not_panic() {
        // We can't reliably write to HKCU in CI, but reading must always work.
        let p = WindowsSystemProxy::new();
        let s = p.snapshot().unwrap();
        // Just check the struct is well-formed.
        let _ = s.was_enabled();
    }
}
