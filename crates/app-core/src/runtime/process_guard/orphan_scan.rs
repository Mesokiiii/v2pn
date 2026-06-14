//! High-level recovery helper: finds every running `sing-box.exe`
//! whose `-D <runtime_dir>` argument matches our directory and
//! taskkills it. The fallback path used at startup when:
//!   * a previous v2pn was an older build without the Job Object
//!   * the OS denied the job assignment (rare admin-boundary edge)
//!   * the state file was deleted by hand or corrupted, so the
//!     PID-based recovery path missed it
//!
//! Errors are logged and never propagated — a startup hiccup must not
//! block the user's main flow.

use super::{enumerate::list_singbox_pids, kill::taskkill_force};

#[cfg(windows)]
use super::inspect::read_process_command_line;

/// Walk the process table for `sing-box.exe`, kill each one whose
/// command line contains `-D <runtime_dir>`. Returns the count of
/// terminated processes.
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
        // Self-skip: don't touch anything from the *current* v2pn —
        // matters only in tests / dev runs where multiple builds
        // coexist. The current v2pn is unlikely to host a sing-box
        // yet at the moment recovery runs, but guard anyway.
        if pid == std::process::id() {
            continue;
        }

        // Best-effort command-line inspection. If we can't read it
        // (different elevation, foreign user) we skip — we wouldn't
        // be allowed to kill it anyway, and erroring on the side of
        // leaving a foreign sing-box alone is the right call.
        #[cfg(windows)]
        {
            let Some(cmdline) = read_process_command_line(pid) else {
                tracing::debug!(target: "process_guard",
                    "PID {pid} sing-box.exe: command line unreadable, skipping");
                continue;
            };
            if !cmdline.to_lowercase().contains(&runtime_str) {
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
            // POSIX path: no cmdline introspection here yet, fall
            // back to state-file PID recovery.
            let _ = (pid, &runtime_str, taskkill_force as fn(u32) -> _);
        }
    }

    if killed > 0 {
        tracing::info!(target: "process_guard",
            "orphan scan terminated {killed} stale sing-box process(es)");
    }
    killed
}
