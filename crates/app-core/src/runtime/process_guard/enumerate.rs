//! Walk the live process table and pick out the PIDs of every
//! `sing-box.exe`. No admin rights required, works on every Windows
//! since XP via `CreateToolhelp32Snapshot`.

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use ::windows::Win32::Foundation::CloseHandle;
    use ::windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// Return the PIDs of every running `sing-box.exe`. Empty vector
    /// on failure (no admin to enumerate, snapshot API errored, …).
    pub fn list_singbox_pids() -> Vec<u32> {
        let mut out = Vec::new();
        // SAFETY: TH32CS_SNAPPROCESS + pid 0 is the documented call
        // to take a system-wide snapshot; the returned handle is
        // closed on the way out via CloseHandle.
        let snap = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(target: "process_guard",
                    "CreateToolhelp32Snapshot failed: {e}");
                return out;
            }
        };
        if snap.is_invalid() {
            return out;
        }

        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        // SAFETY: snap is non-null; `entry` is a stack-allocated,
        // properly sized buffer the API writes into. We honour the
        // documented loop pattern: First, then while Next.
        unsafe {
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    let exe_name = String::from_utf16_lossy(
                        &entry.szExeFile[..entry
                            .szExeFile
                            .iter()
                            .position(|c| *c == 0)
                            .unwrap_or(entry.szExeFile.len())],
                    );
                    if exe_name.eq_ignore_ascii_case("sing-box.exe") {
                        out.push(entry.th32ProcessID);
                    }
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }

        out
    }
}

#[cfg(not(windows))]
mod imp {
    /// We don't ship to non-Windows yet; an empty list is the safe
    /// default — recovery has nothing extra to clean up.
    pub fn list_singbox_pids() -> Vec<u32> {
        Vec::new()
    }
}

pub use imp::list_singbox_pids;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_singbox_pids_does_not_panic() {
        let _ = list_singbox_pids();
    }
}
