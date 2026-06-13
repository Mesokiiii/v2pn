//! sing-box adapters: config building, sanitisation, supervisor.
//!
//! ```text
//!   ProxyProfile  ──┐
//!                   │   build()
//!   ConnectionMode ─┴──▶ sing-box JSON (validated)
//!                                │
//!                                ▼
//!                         supervisor::start
//! ```

pub mod config;
pub mod sanitize;
