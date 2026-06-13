//! OS-level guarantees that the sing-box sidecar never outlives v2pn.
//!
//! ## Why this exists
//!
//! Tokio's `kill_on_drop` only fires if the parent process drops the `Child`
//! handle cleanly. It does **not** fire when:
//!  * v2pn itself is killed with `taskkill /F` / `SIGKILL`
//!  * The system bluescreens / loses power
//!  * A panic during shutdown skips the destructor
//!  * Tauri's runtime is torn down with the supervisor's lock held
//!
//! In all these cases sing-box would happily keep running, holding port 7890,
//! the TUN adapter, and the hijacked DNS — forcing the user to open Task
//! Manager. We use Windows **Job Objects** to make the OS itself enforce the
//! invariant: when the v2pn process handle dies (no matter how), every child
//! we have placed in the job is killed by the kernel.
//!
//! ## Usage
//!
//! ```ignore
//! let guard = ProcessJobGuard::create_kill_on_close()?;
//! // … spawn child with std::process / tokio::process …
//! guard.assign(child_pid)?;
//! ```
//!
//! On non-Windows targets every method is a no-op so the supervisor stays
//! cross-platform.

#[cfg(windows)]
mod imp {
    use std::mem::size_of;

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
    use ::windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use ::windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// Owns a Windows Job Object configured to kill every assigned process
    /// the moment the last handle to the job closes. We hold that handle for
    /// the lifetime of `v2pn.exe`, so any sing-box we put into the job dies
    /// when v2pn itself dies — by graceful exit, panic, kill, or BSOD.
    pub struct ProcessJobGuard {
        // SAFETY: we close the handle in `Drop`. The kernel does not require
        // us to do anything else: closing the last handle triggers the
        // KILL_ON_JOB_CLOSE behaviour automatically.
        handle: HANDLE,
    }

    // The HANDLE in a JobObject is just a kernel pointer; nothing on it is
    // thread-local. Microsoft's docs explicitly allow cross-thread use.
    unsafe impl Send for ProcessJobGuard {}
    unsafe impl Sync for ProcessJobGuard {}

    impl ProcessJobGuard {
        /// Create the job, mark it `kill-on-close`. Idempotent: returns a
        /// fresh job each call. Caller is expected to keep the guard alive
        /// for the *whole* application lifetime.
        pub fn create_kill_on_close() -> std::io::Result<Self> {
            // SAFETY: CreateJobObjectW with both parameters NULL is well-
            // defined; the returned HANDLE is valid until we CloseHandle it.
            let handle = unsafe { CreateJobObjectW(None, None) }
                .map_err(|e| std::io::Error::other(format!("CreateJobObjectW: {e}")))?;
            if handle.is_invalid() {
                return Err(std::io::Error::other("CreateJobObjectW returned NULL"));
            }

            // Fill in the extended limits — KILL_ON_JOB_CLOSE is the bit we
            // care about. Everything else (memory caps etc.) stays at zero.
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: pointer points to a stack-allocated, properly-aligned
            // struct of the size we pass; SetInformationJobObject only reads
            // it. We bail and close the handle if the call fails.
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if let Err(e) = ok {
                // Best-effort cleanup; we're already on an error path.
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

        /// Place the process identified by `pid` into the job. After this
        /// call, the OS guarantees that the process dies as soon as **all**
        /// handles to the job close — i.e. as soon as v2pn itself goes away.
        ///
        /// Errors are non-fatal at the call site: if we can't assign (rare,
        /// usually because the target process already terminated), the
        /// supervisor will fall back to its existing `kill_on_drop` /
        /// `start_kill` paths.
        pub fn assign(&self, pid: u32) -> std::io::Result<()> {
            // We need both PROCESS_SET_QUOTA and PROCESS_TERMINATE to be
            // allowed to put a process into a job object that has KILL on
            // it. The child we just spawned is owned by the same user, so
            // these access bits are granted unconditionally.
            // SAFETY: OpenProcess returns INVALID_HANDLE_VALUE on failure,
            // which we test for. On success the handle is closed before we
            // return, so no leak.
            let proc_handle = unsafe {
                OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)
            }
            .map_err(|e| std::io::Error::other(format!("OpenProcess({pid}): {e}")))?;
            if proc_handle.is_invalid() {
                return Err(std::io::Error::other(format!(
                    "OpenProcess({pid}) returned NULL"
                )));
            }

            // SAFETY: both handles are non-null and valid for the duration
            // of the call. We close the process handle right after.
            let assign_res = unsafe { AssignProcessToJobObject(self.handle, proc_handle) };
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
            // SAFETY: we own the handle; closing it triggers KILL_ON_JOB_CLOSE.
            unsafe {
                let _ = CloseHandle(self.handle);
            }
            tracing::info!(target: "process_guard",
                "Job Object handle closed — any assigned child will be terminated by the kernel");
        }
    }

    /// Best-effort hard kill of a Windows process by PID. Used by the
    /// recovery path on startup when we discover a stale `child_pid` in the
    /// state file.
    ///
    /// PID-reuse defence: between the moment we recorded `pid` (in our
    /// state file or by walking the process list) and right now, the OS
    /// could have terminated the original process and reassigned that PID
    /// to a completely unrelated one — perhaps even one we have no
    /// business killing (a system service, the user's editor, ...). Before
    /// we send the terminate signal, we cross-check the exe path on the
    /// handle we just opened: if it isn't `sing-box.exe`, we refuse and
    /// log a warning. That's the only race-free way to do this on
    /// Windows: handles snapshot the process identity at OpenProcess time.
    pub fn taskkill_force(pid: u32) -> std::io::Result<()> {
        use ::windows::Win32::System::Threading::{
            TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        };
        use ::windows::Win32::System::ProcessStatus::GetModuleFileNameExW;

        // Open with both QUERY_LIMITED_INFORMATION (for the exe-name
        // re-check) and TERMINATE (for the actual kill). The handle, once
        // we have it, identifies the process for life — even if the OS
        // reuses the PID right after this call returns, *our* handle
        // still points to the same process we vetted.
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

        // Identity re-check: the handle must point at sing-box.exe. Any
        // mismatch (system process, foreign tool, freshly-recycled PID)
        // → refuse and surface a clear error so it shows up in the log
        // instead of silently mis-killing a victim.
        // SAFETY: GetModuleFileNameExW writes up to `buf.len()` u16s and
        // returns the count; we trim to the returned length before
        // converting to UTF-16. The handle is closed below regardless.
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
        let exe_path_lc = exe_path.to_lowercase();
        if !exe_path_lc.ends_with("sing-box.exe") {
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

    /// Walk the live process table and return the PIDs of every running
    /// `sing-box.exe`. We use `CreateToolhelp32Snapshot` — no admin rights,
    /// works on every Windows since XP, and gives us just the executable
    /// basename which is all we need to filter by.
    pub fn list_singbox_pids() -> Vec<u32> {
        use ::windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
            PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };

        let mut out = Vec::new();
        // SAFETY: TH32CS_SNAPPROCESS + pid 0 is the documented call to take
        // a system-wide snapshot; the returned handle is closed on the way
        // out via CloseHandle.
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

        // SAFETY: snap is non-null; `entry` is a stack-allocated, properly
        // sized buffer the API writes into. We honour the documented loop
        // pattern: First, then while Next.
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

    /// Read a process's command line by walking its PEB. This is how Task
    /// Manager / Process Explorer / Process Hacker do it, and how the only
    /// non-WMI option in Rust libraries (sysinfo, etc.) does it under the
    /// hood. Used by the orphan scanner to confirm a `sing-box.exe`
    /// belongs to *our* v2pn install before terminating it.
    ///
    /// Returns `None` on access denied (sing-box was started by another
    /// user or in a different elevation context — we wouldn't be able to
    /// kill it anyway, so the caller can safely skip it).
    ///
    /// Architecture safety: v2pn ships as x64-only and the offsets below
    /// (`PEB_PROCESS_PARAMETERS_OFFSET=0x20`, `PARAMS_COMMAND_LINE_OFFSET=0x70`)
    /// are valid only for native x64 PEBs. If the target turns out to be
    /// a 32-bit (WoW64) sing-box, those offsets would point at random
    /// bytes; we detect that case via `IsWow64Process2` and return `None`
    /// rather than risk reading mismapped memory. Sing-box itself ships
    /// only as x64 on Windows, so a WoW64 hit means we're looking at
    /// some unrelated binary that happens to share the basename — the
    /// orphan scanner will skip it correctly.
    pub fn read_process_command_line(pid: u32) -> Option<String> {
        use std::ffi::c_void;
        use std::mem::zeroed;

        use ::windows::Wdk::System::Threading::{
            NtQueryInformationProcess, ProcessBasicInformation,
        };
        use ::windows::Win32::System::Threading::{
            IsWow64Process2, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
        };
        use ::windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_UNKNOWN;
        use ::windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;

        // We need both VM_READ (to dereference the PEB) and
        // QUERY_LIMITED_INFORMATION (to call NtQueryInformationProcess).
        // Failure here is expected for elevated/foreign sing-boxes — we
        // map it to None and let the caller skip the kill.
        let h = unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
        }
        .ok()?;
        if h.is_invalid() {
            return None;
        }

        // Architecture sanity check. `process_machine` is non-zero (i.e.
        // the process is running under WoW64 emulation) ⇒ 32-bit binary,
        // PEB layout differs, our offsets won't apply. Return None.
        let mut process_machine: ::windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE = IMAGE_FILE_MACHINE_UNKNOWN;
        let mut native_machine: ::windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE = IMAGE_FILE_MACHINE_UNKNOWN;
        // SAFETY: both out-pointers are stack-allocated u16s; the syscall
        // either writes or fails (.is_err is checked). On any error we
        // bail and close the handle.
        let arch_ok = unsafe {
            IsWow64Process2(h, &mut process_machine, Some(&mut native_machine))
        };
        if arch_ok.is_err() {
            unsafe { let _ = CloseHandle(h); }
            return None;
        }
        if process_machine != IMAGE_FILE_MACHINE_UNKNOWN {
            tracing::debug!(target: "process_guard",
                "PID {pid} is WoW64 ({:?}); skipping PEB read", process_machine.0);
            unsafe { let _ = CloseHandle(h); }
            return None;
        }

        // PROCESS_BASIC_INFORMATION layout: undocumented across Windows
        // versions in name, but field offsets have been stable since XP.
        // We only need the PebBaseAddress field, the second pointer.
        #[repr(C)]
        struct PbiPartial {
            _exit_status: i32,
            peb_base: *mut c_void,
            _affinity_mask: usize,
            _base_priority: i32,
            _unique_pid: usize,
            _inherited_pid: usize,
        }
        let mut pbi: PbiPartial = unsafe { zeroed() };
        let mut ret_len: u32 = 0;

        // SAFETY: pbi is a stack-allocated, correctly-sized struct. The
        // NTSTATUS return is consulted via .is_ok().
        let ok = unsafe {
            NtQueryInformationProcess(
                h,
                ProcessBasicInformation,
                &mut pbi as *mut _ as *mut c_void,
                size_of::<PbiPartial>() as u32,
                &mut ret_len,
            )
        };
        if ok.is_err() || pbi.peb_base.is_null() {
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }

        // Dereference the PEB to find ProcessParameters. Layout offsets:
        //   x64: PEB.ProcessParameters at 0x20
        //        RTL_USER_PROCESS_PARAMETERS.CommandLine at 0x70
        //        UNICODE_STRING { u16 Length, u16 MaxLen, ptr Buffer }
        // These offsets are stable from Windows 7 through 11. They're
        // hardcoded here rather than via bindgen because the structs are
        // marked semi-internal in MSDN and pulling in `ntapi` to avoid two
        // numbers is overkill.
        const PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
        const PARAMS_COMMAND_LINE_OFFSET: usize = 0x70;

        // Read &PEB.ProcessParameters (a pointer).
        let mut params_ptr: *mut c_void = std::ptr::null_mut();
        // SAFETY: ReadProcessMemory checks bounds; on failure we close
        // the handle and return None. peb_base + offset is well within
        // the kernel-mapped PEB page.
        let read_ok = unsafe {
            ReadProcessMemory(
                h,
                pbi.peb_base.byte_add(PEB_PROCESS_PARAMETERS_OFFSET),
                &mut params_ptr as *mut _ as *mut _,
                size_of::<*mut c_void>(),
                None,
            )
        };
        if read_ok.is_err() || params_ptr.is_null() {
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }

        // Read CommandLine UNICODE_STRING { u16 len, u16 max, ptr buf }.
        #[repr(C)]
        struct UnicodeString {
            length: u16,
            maximum_length: u16,
            _padding: u32,
            buffer: *mut u16,
        }
        let mut us: UnicodeString = unsafe { zeroed() };
        let read_ok = unsafe {
            ReadProcessMemory(
                h,
                params_ptr.byte_add(PARAMS_COMMAND_LINE_OFFSET),
                &mut us as *mut _ as *mut _,
                size_of::<UnicodeString>(),
                None,
            )
        };
        if read_ok.is_err() || us.length == 0 || us.buffer.is_null() {
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }

        // Read the actual UTF-16 buffer.
        let utf16_len = (us.length as usize) / 2;
        // Cap at 32 KiB to defend against corrupt/oversized data — sing-box
        // command lines are well under 4 KiB in practice.
        let utf16_len = utf16_len.min(16 * 1024);
        let mut buf = vec![0u16; utf16_len];
        let read_ok = unsafe {
            ReadProcessMemory(
                h,
                us.buffer as *const c_void,
                buf.as_mut_ptr() as *mut _,
                utf16_len * 2,
                None,
            )
        };
        unsafe {
            let _ = CloseHandle(h);
        }
        if read_ok.is_err() {
            return None;
        }

        Some(String::from_utf16_lossy(&buf))
    }
}

#[cfg(not(windows))]
mod imp {
    /// On non-Windows we don't have Job Objects; the Tokio
    /// `Command::kill_on_drop(true)` and POSIX `SIGTERM` semantics are
    /// already well-defined. The struct exists so the supervisor's API is
    /// identical across platforms.
    pub struct ProcessJobGuard;

    impl ProcessJobGuard {
        pub fn create_kill_on_close() -> std::io::Result<Self> {
            Ok(Self)
        }
        pub fn assign(&self, _pid: u32) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub fn taskkill_force(pid: u32) -> std::io::Result<()> {
        // Best-effort SIGKILL via libc. Errors are folded into io::Error.
        // SAFETY: kill(2) with SIGKILL is well-defined for any signed PID;
        // a non-existent PID just returns ESRCH which we propagate.
        let r = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        if r == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    pub fn list_singbox_pids() -> Vec<u32> {
        // We don't ship to non-Windows yet; an empty list is the safe
        // default — recovery has nothing extra to clean up.
        Vec::new()
    }
}

pub use imp::*;

/// High-level orphan scanner. Walks the live process table for every
/// running `sing-box.exe`, asks each one for its command line, and kills
/// the ones whose `-D <runtime_dir>` argument matches our directory. This
/// is the *fallback* path: under normal operation the kill-on-close Job
/// Object on the previous run already handled cleanup before we got here.
/// We run this anyway for the corner cases where:
///   * the previous v2pn was an older build without the Job Object
///   * the OS denied the job assignment (rare admin-boundary edge case)
///   * the state file was deleted by hand or corrupted, so the PID-based
///     recovery path missed it
///
/// Returns the count of processes terminated. Errors are logged but never
/// propagated — a startup hiccup must not block the user's main flow.
pub fn kill_orphan_singboxes_for_runtime(runtime_dir: &std::path::Path) -> usize {
    let runtime_str = runtime_dir.to_string_lossy().to_lowercase();
    if runtime_str.is_empty() {
        return 0;
    }

    let pids = list_singbox_pids();
    if pids.is_empty() {
        return 0;
    }

    let mut killed = 0;
    for pid in pids {
        // Self-skip: don't kill anything from the *current* v2pn — only
        // matters in tests / dev runs where multiple builds coexist.
        // The current v2pn is unlikely to host a sing-box yet at the
        // moment recovery runs, but guard anyway.
        if pid == std::process::id() {
            continue;
        }

        // Best-effort command-line inspection. If we can't read it
        // (different elevation, foreign user) we skip — we wouldn't be
        // allowed to kill it anyway, and erroring on the side of leaving
        // a foreign sing-box alone is the right call.
        #[cfg(windows)]
        {
            let Some(cmdline) = read_process_command_line(pid) else {
                tracing::debug!(target: "process_guard",
                    "PID {pid} sing-box.exe: command line unreadable, skipping");
                continue;
            };
            let cmdline_lc = cmdline.to_lowercase();
            // Match a sing-box invocation pinned to *our* runtime_dir.
            // The supervisor always passes `-D <runtime_dir>`; orphan
            // detection is only valid if that exact path appears.
            if !cmdline_lc.contains(&runtime_str) {
                tracing::debug!(target: "process_guard",
                    "PID {pid} sing-box.exe: foreign cmdline, skipping ({cmdline})");
                continue;
            }

            tracing::warn!(target: "process_guard",
                "PID {pid} sing-box.exe is an orphan from our runtime_dir — terminating");
            match taskkill_force(pid) {
                Ok(()) => killed += 1,
                Err(e) => tracing::error!(target: "process_guard",
                    "taskkill_force({pid}) failed: {e}"),
            }
        }
        #[cfg(not(windows))]
        {
            // Without command-line introspection we can't safely match by
            // runtime_dir on POSIX from this minimal helper. Recovery on
            // non-Windows still relies on the state-file PID path.
            let _ = (pid, &runtime_str);
        }
    }

    if killed > 0 {
        tracing::info!(target: "process_guard",
            "orphan scan terminated {killed} stale sing-box process(es)");
    }
    killed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_drop_job_guard_is_safe() {
        // Just exercise the construction path. We can't assign a real
        // process here without spawning one — the more interesting tests
        // live in the supervisor integration suite.
        let g = ProcessJobGuard::create_kill_on_close().expect("create");
        drop(g);
    }

    #[test]
    fn list_singbox_pids_does_not_panic() {
        let _ = list_singbox_pids();
    }
}
