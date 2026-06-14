//! OS-level guarantees that the sing-box sidecar never outlives v2pn.
//!
//! ## Why this exists
//!
//! Tokio's `kill_on_drop` only fires if the parent drops the `Child`
//! handle cleanly. It does **not** fire when:
//!  * v2pn itself is killed with `taskkill /F` / `SIGKILL`
//!  * The system bluescreens / loses power
//!  * A panic during shutdown skips the destructor
//!  * Tauri's runtime is torn down with the supervisor's lock held
//!
//! In all these cases sing-box would happily keep running, holding
//! port 7890, the TUN adapter, and the hijacked DNS — forcing the
//! user to open Task Manager. This module gives the supervisor the
//! tools to make that impossible.
//!
//! ## Layout
//!
//! Each concern is its own submodule with a single-paragraph
//! responsibility statement. The top-level facade re-exports the
//! public surface so callers can keep saying
//! `app_core::process_guard::ProcessJobGuard` without caring how
//! the implementation is sliced.
//!
//!  - [`job`]         — Windows Job Object, kill-on-close lifecycle.
//!  - [`kill`]        — `taskkill_force(pid)` with PID-reuse defence.
//!  - [`enumerate`]   — `list_singbox_pids()` via Toolhelp32.
//!  - [`inspect`]     — `read_process_command_line(pid)` via PEB walk.
//!  - [`orphan_scan`] — high-level "kill any stray sing-box that
//!                      points at our runtime_dir".
//!
//! ## Usage
//!
//! ```ignore
//! let guard = ProcessJobGuard::create_kill_on_close()?;
//! // … spawn child with std::process / tokio::process …
//! guard.assign(child_pid)?;
//! ```
//!
//! On non-Windows targets every primitive is a no-op / stub so the
//! supervisor stays cross-platform.

mod enumerate;
mod inspect;
mod job;
mod kill;
mod orphan_scan;

pub use enumerate::list_singbox_pids;
pub use inspect::read_process_command_line;
pub use job::ProcessJobGuard;
pub use kill::taskkill_force;
pub use orphan_scan::kill_orphan_singboxes_for_runtime;
