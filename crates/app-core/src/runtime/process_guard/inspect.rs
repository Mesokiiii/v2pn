//! Read another process's command line by walking its PEB.
//!
//! This is how Task Manager / Process Explorer / Process Hacker do it,
//! and how the only non-WMI option in Rust libraries (sysinfo, etc.)
//! does it under the hood. Used by the orphan scanner to confirm a
//! `sing-box.exe` belongs to *our* v2pn install (its `-D <runtime_dir>`
//! argument matches ours) before terminating it.
//!
//! Architecture safety: v2pn ships as x64-only and the offsets below
//! are valid only for native x64 PEBs. We detect WoW64 / non-native
//! processes via `IsWow64Process2` and return `None` rather than risk
//! reading mismapped memory. Sing-box ships only as x64 on Windows, so
//! a WoW64 hit means we're looking at some unrelated binary that
//! happens to share the basename — the orphan scanner will skip it.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};

    use ::windows::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessBasicInformation,
    };
    use ::windows::Win32::Foundation::CloseHandle;
    use ::windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use ::windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_UNKNOWN;
    use ::windows::Win32::System::Threading::{
        IsWow64Process2, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    /// PEB struct offsets, stable from Windows 7 through 11 on x64.
    /// Hardcoded rather than via bindgen because the structs are
    /// marked semi-internal in MSDN and pulling in `ntapi` to avoid
    /// two numbers is overkill.
    const PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
    const PARAMS_COMMAND_LINE_OFFSET: usize = 0x70;

    /// Cap on the command-line UTF-16 buffer we allocate. sing-box
    /// command lines are well under 4 KiB in practice; this is the
    /// defense-in-depth limit against corrupt / oversized data.
    const MAX_CMDLINE_UTF16_UNITS: usize = 16 * 1024;

    /// Read a process's command line. Returns `None` on access denied
    /// (foreign user, different elevation), on architecture mismatch
    /// (WoW64), or on any FFI failure — the caller treats the
    /// nullable result as "skip this PID".
    pub fn read_process_command_line(pid: u32) -> Option<String> {
        // We need both VM_READ (to dereference the PEB) and
        // QUERY_LIMITED_INFORMATION (for NtQueryInformationProcess).
        let h = unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
        }
        .ok()?;
        if h.is_invalid() {
            return None;
        }

        // Architecture sanity check. `process_machine` non-zero ⇒
        // 32-bit binary running under WoW64; PEB layout differs, our
        // offsets won't apply.
        let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
        let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;
        // SAFETY: both out-pointers are stack-allocated; the syscall
        // either writes or fails (.is_err is checked).
        let arch_ok = unsafe {
            IsWow64Process2(h, &mut process_machine, Some(&mut native_machine))
        };
        if arch_ok.is_err() {
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }
        if process_machine != IMAGE_FILE_MACHINE_UNKNOWN {
            tracing::debug!(target: "process_guard",
                "PID {pid} is WoW64 ({:?}); skipping PEB read", process_machine.0);
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }

        // PROCESS_BASIC_INFORMATION layout: undocumented across
        // Windows versions in name, but field offsets have been
        // stable since XP. We only need PebBaseAddress (2nd pointer).
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

        // SAFETY: pbi is a stack-allocated, correctly sized struct.
        // The NTSTATUS return is consulted via .is_ok().
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

        // Read &PEB.ProcessParameters (a pointer).
        let mut params_ptr: *mut c_void = std::ptr::null_mut();
        // SAFETY: ReadProcessMemory checks bounds; on failure we
        // close the handle and return None. peb_base + offset is
        // well within the kernel-mapped PEB page.
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
        let utf16_len = ((us.length as usize) / 2).min(MAX_CMDLINE_UTF16_UNITS);
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
    /// On non-Windows we don't ship a PEB-walker. The orphan scanner
    /// has no way to disambiguate "ours" vs "someone else's" sing-box
    /// without reading /proc; it falls back to the state-file PID
    /// path instead. Empty result mirrors that.
    pub fn read_process_command_line(_pid: u32) -> Option<String> {
        None
    }
}

pub use imp::read_process_command_line;
