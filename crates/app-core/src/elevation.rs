//! Privilege-elevation helpers.
//!
//! TUN-mode (Wintun on Windows) needs `SeLoadDriverPrivilege`, which only an
//! elevated process has. We expose two Rust APIs to the UI:
//!
//!   * [`is_elevated`]    — fast probe, used to decide whether to enable the
//!                          TUN switch or surface a "needs admin" hint.
//!   * [`restart_as_admin`] — re-launches the same exe via Windows
//!                          `ShellExecute("runas", ...)`, which raises a
//!                          standard UAC prompt. We exit on success so only
//!                          the elevated copy survives.

#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevationStatus {
    pub elevated: bool,
    /// Distinct from `elevated` — only available on Windows. Other OSes
    /// always report `true` here so the UI can hide the prompt.
    pub supported: bool,
}

#[cfg(windows)]
pub fn is_elevated() -> ElevationStatus {
    use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
    use ::windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use ::windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return ElevationStatus { elevated: false, supported: true };
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            ::windows::Win32::Security::TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size,
            &mut returned,
        );
        let _ = CloseHandle(token);
        ElevationStatus {
            elevated: ok.is_ok() && elevation.TokenIsElevated != 0,
            supported: true,
        }
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> ElevationStatus {
    // We don't ship to non-Windows yet. Pretend "elevated" so TUN UI is enabled
    // (and the underlying tun setup will return its own real error if needed).
    ElevationStatus { elevated: true, supported: false }
}

/// Re-launch the current executable with a UAC prompt. Returns Ok(()) only
/// if the elevated child was actually spawned — caller is expected to exit
/// the process immediately after.
#[cfg(windows)]
pub fn restart_as_admin() -> std::io::Result<()> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use ::windows::core::PCWSTR;
    use ::windows::Win32::UI::Shell::{ShellExecuteW, SE_ERR_ACCESSDENIED};
    use ::windows::Win32::UI::WindowsAndMessaging::SW_NORMAL;

    let exe = std::env::current_exe()?;
    let exe_w: Vec<u16> = OsString::from(exe).encode_wide().chain(Some(0)).collect();
    let verb_w: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();

    // Best-effort: pass through the original args.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let joined = args.join(" ");
    let args_w: Vec<u16> = joined.encode_utf16().chain(Some(0)).collect();

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            if args_w.len() > 1 { PCWSTR(args_w.as_ptr()) } else { PCWSTR(ptr::null()) },
            None,
            SW_NORMAL,
        )
    };

    // ShellExecuteW returns an HINSTANCE > 32 on success.
    let code = result.0 as isize;
    if code <= 32 {
        return Err(match code as u32 {
            // 5 = ERROR_ACCESS_DENIED, also used when user clicks "No" on UAC.
            n if n == SE_ERR_ACCESSDENIED || n == 5 => {
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "UAC elevation declined")
            }
            other => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("ShellExecuteW failed with code {other}"),
            ),
        });
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn restart_as_admin() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "elevation only implemented on Windows",
    ))
}
