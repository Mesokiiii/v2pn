//! Network-layer concerns: talking to sing-box's clash API,
//! emergency network cleanup, latency probing, Wintun adapter
//! housekeeping.
//!
//! Modules:
//!  - [`clash_api`]       — typed HTTP client for sing-box's
//!                          experimental clash API (auth, switch,
//!                          probe, /connections, /version).
//!  - [`outbound_health`] — thin wrapper over [`clash_api`] that
//!                          shapes results for the UI.
//!  - [`probe`]           — TCP-level latency probe used to rank
//!                          servers in the sidebar.
//!  - [`network_repair`]  — "fix my network" emergency cleanup
//!                          (proxy reset + DNS/ARP flush + Wintun
//!                          sweep).
//!  - [`wintun_cleanup`]  — remove stale Wintun adapters left by a
//!                          previous crashed sing-box.

pub mod clash_api;
pub mod network_repair;
pub mod outbound_health;
pub mod probe;
pub mod wintun_cleanup;
