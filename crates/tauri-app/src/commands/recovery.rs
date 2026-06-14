//! Startup recovery. Cleans up after a previous v2pn run that crashed
//! / was killed / lost power, BEFORE the supervisor tries to spawn its
//! own sing-box child. Two layered fallbacks:
//!
//!  1. State-file recovery: if `<runtime_dir>/active-state.json`
//!     exists, restore the OS proxy from the saved snapshot and
//!     force-kill the recorded child PID.
//!  2. Process-table orphan scan: even without a state file, walk the
//!     process table for `sing-box.exe` whose `-D <runtime_dir>`
//!     argument matches ours and kill them. Catches the case where a
//!     prior run was killed before it could write its state file.

use app_core::state_guard::{recover_orphaned_state, RecoveryOutcome};

/// Run startup recovery. Logs the outcome and returns it for the UI
/// to display. Always returns — never propagates an error, because
/// "I couldn't recover" is itself recoverable on the next launch.
pub fn run_startup_recovery(state_dir: &std::path::Path) -> RecoveryOutcome {
    let runtime_dir = state_dir;

    // Step 1: state-file recovery. Standard case: the previous v2pn
    // left a guard file behind, so we restore the OS proxy and
    // force-kill the recorded child PID.
    let outcome = match recover_orphaned_state(state_dir) {
        Ok(outcome) => {
            match &outcome {
                RecoveryOutcome::NothingToDo => {}
                RecoveryOutcome::OwnedByLiveProcess { pid } => {
                    tracing::warn!(target: "recovery",
                        "another v2pn (PID {pid}) holds the proxy; not touching");
                }
                RecoveryOutcome::Recovered { applied, .. } => {
                    tracing::info!(target: "recovery",
                        "recovered from previous crash, restored proxy (was {:?})", applied);
                }
            }
            outcome
        }
        Err(e) => {
            tracing::error!(target: "recovery", "recovery failed: {e}");
            RecoveryOutcome::NothingToDo
        }
    };

    // Step 2: orphan-process scan. Independent fallback for the cases
    // where the state file was deleted by hand, the previous v2pn
    // never got a chance to write it, or the kill-on-close Job Object
    // didn't fire. Walks the process table, kills any sing-box.exe
    // whose -D argument points at our runtime_dir. Skipped if Step 1
    // says another v2pn instance is the legitimate owner — in that
    // case the foreign sing-box belongs to it, not to us.
    if !matches!(outcome, RecoveryOutcome::OwnedByLiveProcess { .. }) {
        let killed =
            app_core::process_guard::kill_orphan_singboxes_for_runtime(runtime_dir);
        if killed > 0 {
            // Wintun grace period: the kernel queues adapter teardown
            // for a few hundred ms after the holding process exits.
            // Without this sleep the next supervisor::start() may race
            // the cleanup and bail with "Element not found" / "Cannot
            // create a file".
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
    }

    outcome
}
