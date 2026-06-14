//! Windows Job Object kill-on-close guard. Owns the OS-level handle that
//! makes the kernel terminate every assigned child the instant we
//! disappear (graceful exit, panic, kill -9, BSOD, power loss). The
//! single strongest guarantee against orphan sing-boxes.

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
    use ::windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use ::windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// Owns a Windows Job Object configured to kill every assigned process
    /// the moment the last handle to the job closes. We hold that handle
    /// for the lifetime of `v2pn.exe`, so any sing-box we put into the
    /// job dies when v2pn itself dies — by graceful exit, panic, kill,
    /// or BSOD.
    pub struct ProcessJobGuard {
        // SAFETY: we close the handle in `Drop`. The kernel does not
        // require us to do anything else: closing the last handle
        // triggers KILL_ON_JOB_CLOSE behaviour automatically.
        handle: HANDLE,
    }

    // The HANDLE in a JobObject is just a kernel pointer; nothing on it
    // is thread-local. Microsoft's docs explicitly allow cross-thread use.
    unsafe impl Send for ProcessJobGuard {}
    unsafe impl Sync for ProcessJobGuard {}

    impl ProcessJobGuard {
        /// Create the job, mark it `kill-on-close`. Caller must keep the
        /// guard alive for the *whole* application lifetime — dropping
        /// it terminates every assigned process via the kernel callback.
        pub fn create_kill_on_close() -> std::io::Result<Self> {
            // SAFETY: CreateJobObjectW with both parameters NULL is
            // well-defined; the returned HANDLE is valid until we
            // CloseHandle it.
            let handle = unsafe { CreateJobObjectW(None, None) }
                .map_err(|e| std::io::Error::other(format!("CreateJobObjectW: {e}")))?;
            if handle.is_invalid() {
                return Err(std::io::Error::other("CreateJobObjectW returned NULL"));
            }

            // Fill in the extended limits — KILL_ON_JOB_CLOSE is the
            // bit we care about. Everything else (memory caps, etc.)
            // stays at zero.
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: pointer points to a stack-allocated, properly
            // aligned struct of the size we pass; SetInformationJobObject
            // only reads it. We bail and close the handle on failure.
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if let Err(e) = ok {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return Err(std::io::Error::other(format!(
                    "SetInformationJobObject: {e}"
                )));
            }

            tracing::info!(target: "process_guard",
                "Job Object created (kill-on-close enabled, handle={:#x})",
                handle.0 as usize);

            Ok(Self { handle })
        }

        /// Place the process identified by `pid` into the job. After
        /// this call, the OS guarantees the process dies as soon as
        /// **all** handles to the job close — i.e. as soon as v2pn
        /// itself goes away.
        pub fn assign(&self, pid: u32) -> std::io::Result<()> {
            // We need both PROCESS_SET_QUOTA and PROCESS_TERMINATE to
            // be allowed to put a process into a kill-on-close job.
            // SAFETY: OpenProcess returns INVALID_HANDLE_VALUE on
            // failure, which we test for. On success the handle is
            // closed before we return.
            let proc_handle = unsafe {
                OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)
            }
            .map_err(|e| std::io::Error::other(format!("OpenProcess({pid}): {e}")))?;
            if proc_handle.is_invalid() {
                return Err(std::io::Error::other(format!(
                    "OpenProcess({pid}) returned NULL"
                )));
            }

            // SAFETY: both handles are non-null and valid for the
            // duration of the call. We close the process handle right
            // after.
            let assign_res =
                unsafe { AssignProcessToJobObject(self.handle, proc_handle) };
            unsafe {
                let _ = CloseHandle(proc_handle);
            }

            match assign_res {
                Ok(()) => {
                    tracing::info!(target: "process_guard",
                        "child PID {pid} assigned to kill-on-close job");
                    Ok(())
                }
                Err(e) => Err(std::io::Error::other(format!(
                    "AssignProcessToJobObject({pid}): {e}"
                ))),
            }
        }
    }

    impl Drop for ProcessJobGuard {
        fn drop(&mut self) {
            // SAFETY: we own the handle; closing it triggers
            // KILL_ON_JOB_CLOSE.
            unsafe {
                let _ = CloseHandle(self.handle);
            }
            tracing::info!(target: "process_guard",
                "Job Object handle closed — any assigned child will be terminated by the kernel");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// On non-Windows we don't have Job Objects; Tokio
    /// `Command::kill_on_drop(true)` + POSIX `SIGTERM` semantics are
    /// already well-defined. The struct exists so the supervisor's API
    /// is identical across platforms.
    pub struct ProcessJobGuard;

    impl ProcessJobGuard {
        pub fn create_kill_on_close() -> std::io::Result<Self> {
            Ok(Self)
        }
        pub fn assign(&self, _pid: u32) -> std::io::Result<()> {
            Ok(())
        }
    }
}

pub use imp::ProcessJobGuard;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_drop_job_guard_is_safe() {
        // Just exercise the construction path. Real assign / kill
        // tests live in the supervisor integration suite — we can't
        // spawn a process from this unit test without making it slow
        // and platform-specific.
        let g = ProcessJobGuard::create_kill_on_close().expect("create");
        drop(g);
    }
}
