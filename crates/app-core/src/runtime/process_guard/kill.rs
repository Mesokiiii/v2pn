//! Force-kill a process by PID with PID-reuse defence: the handle we
//! open snapshots the process identity at OpenProcess time, then we
//! verify the exe path before issuing TerminateProcess. If the OS
//! reused the PID for some unrelated victim between us recording it
//! and us trying to kill, we refuse and log a warning — never
//! mis-terminate.

#[cfg(windows)]
mod imp {
    use ::windows::Win32::Foundation::CloseHandle;
    use ::windows::Win32::System::ProcessStatus::GetModuleFileNameExW;
    use ::windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    /// Best-effort hard kill of a Windows process by PID. Used by the
    /// recovery path on startup when we discover a stale `child_pid`
    /// in the state file.
    ///
    /// PID-reuse defence: between the moment we recorded `pid` (in our
    /// state file or by walking the process list) and right now, the
    /// OS could have terminated the original process and reassigned
    /// that PID to a completely unrelated one — perhaps even one we
    /// have no business killing (a system service, the user's editor,
    /// etc.). Before we send the terminate signal, we cross-check the
    /// exe path on the handle we just opened: if it isn't
    /// `sing-box.exe`, we refuse and log a warning. That's the only
    /// race-free way to do this on Windows: handles snapshot the
    /// process identity at `OpenProcess` time.
    pub fn taskkill_force(pid: u32) -> std::io::Result<()> {
        // Open with both QUERY_LIMITED_INFORMATION (for the exe-name
        // re-check) and TERMINATE (for the actual kill). The handle,
        // once we have it, identifies the process for life — even if
        // the OS reuses the PID right after this call returns, *our*
        // handle still points to the same process we vetted.
        let h = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                false,
                pid,
            )
        }
        .map_err(|e| std::io::Error::other(format!("OpenProcess({pid}): {e}")))?;
        if h.is_invalid() {
            return Err(std::io::Error::other(format!(
                "OpenProcess({pid}) returned NULL"
            )));
        }

        // Identity re-check: the handle must point at sing-box.exe.
        // Any mismatch (system process, foreign tool, freshly recycled
        // PID) → refuse and surface a clear error.
        // SAFETY: GetModuleFileNameExW writes up to `buf.len()` u16s
        // and returns the count; we trim to the returned length
        // before converting to UTF-16. The handle is closed below
        // regardless.
        let mut buf = [0u16; 1024];
        let n = unsafe { GetModuleFileNameExW(Some(h), None, &mut buf) };
        if n == 0 {
            unsafe {
                let _ = CloseHandle(h);
            }
            return Err(std::io::Error::other(format!(
                "GetModuleFileNameExW({pid}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        let exe_path = String::from_utf16_lossy(&buf[..n as usize]);
        if !exe_path.to_lowercase().ends_with("sing-box.exe") {
            unsafe {
                let _ = CloseHandle(h);
            }
            tracing::warn!(target: "process_guard",
                "PID {pid} no longer points to sing-box.exe (now: {exe_path}); refusing to terminate");
            return Err(std::io::Error::other(format!(
                "PID {pid} reused for non-sing-box process ({exe_path}); kill refused"
            )));
        }

        // Identity confirmed. Exit code 137 mirrors POSIX SIGKILL — a
        // breadcrumb in the audit log that this kill came from us.
        // SAFETY: handle is valid and we have PROCESS_TERMINATE rights.
        let res = unsafe { TerminateProcess(h, 137) };
        unsafe {
            let _ = CloseHandle(h);
        }
        res.map_err(|e| std::io::Error::other(format!("TerminateProcess({pid}): {e}")))
    }
}

#[cfg(not(windows))]
mod imp {
    /// Best-effort SIGKILL via libc. Errors are folded into io::Error.
    pub fn taskkill_force(pid: u32) -> std::io::Result<()> {
        // SAFETY: kill(2) with SIGKILL is well-defined for any
        // signed PID; a non-existent PID just returns ESRCH which we
        // propagate.
        let r = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        if r == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

pub use imp::taskkill_force;
