//! Sing-box runtime lifecycle: spawning, supervising, validating,
//! reaping. Everything that owns or observes the child process lives
//! here.
//!
//! Modules:
//!  - [`supervisor`]      — primary lifecycle FSM (Idle → Starting →
//!                          Connected → Stopping / Failed).
//!  - [`watchdog`]        — restarts sing-box on unexpected death.
//!  - [`state_validator`] — clash-API preflight (config sanity check
//!                          via /version after Starting).
//!  - [`port_pick`]       — pick a free loopback port if the requested
//!                          one is busy.
//!  - [`process_guard`]   — Job-Object based reap-on-parent-exit
//!                          guarantee + orphan-process scanner.

pub mod port_pick;
pub mod process_guard;
pub mod state_validator;
pub mod supervisor;
pub mod watchdog;
