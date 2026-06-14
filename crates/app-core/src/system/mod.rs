//! OS-level interactions: anything that touches Windows-specific
//! APIs, the registry, hardware identifiers, power events, the
//! system proxy backend, or our on-disk state-recovery file.
//!
//! Modules:
//!  - [`elevation`]   — UAC integrity-level query + relaunch-as-admin.
//!  - [`hwid`]        — stable machine ID, used to namespace the
//!                      OS-keyring entry that holds the profile-DB
//!                      encryption key.
//!  - [`power`]       — Windows power-broadcast hooks
//!                      (suspend / resume).
//!  - [`state_guard`] — RAII guard around the OS-proxy snapshot,
//!                      mirrored to disk so the next launch can
//!                      recover from a hard crash.
//!  - [`sys_proxy`]   — pluggable backend for reading/writing the
//!                      OS proxy registry / settings.

pub mod elevation;
pub mod hwid;
pub mod power;
pub mod state_guard;
pub mod sys_proxy;
