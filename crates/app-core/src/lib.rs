//! v2pn core: business logic with no UI dependencies.
//!
//! # Layout
//!
//! Internal layout is grouped by domain — `model` / `runtime` /
//! `net` / `system` — plus two protocol-specific subtrees that own
//! their own scope (`singbox`, `subscription`):
//!
//! ```text
//! app_core
//! ├── error                    // CoreError + CoreResult
//! ├── model                    // ProxyProfile, typed newtypes
//! ├── runtime                  // sing-box lifecycle (supervisor,
//! │                            //   watchdog, state_validator,
//! │                            //   port_pick, process_guard)
//! ├── net                      // clash_api, outbound_health,
//! │                            //   probe, network_repair,
//! │                            //   wintun_cleanup
//! ├── system                   // elevation, hwid, power,
//! │                            //   state_guard, sys_proxy
//! ├── singbox                  // config builder + sanitiser
//! └── subscription             // fetch + format-sniff + parsers
//! ```
//!
//! For a frictionless callsite the leaf modules are also re-exported
//! at the crate root, so external code can write the short
//! `app_core::supervisor::Supervisor` instead of the full
//! `app_core::runtime::supervisor::Supervisor`. Both paths resolve
//! to the same module.

pub mod error;

pub mod model;
pub mod net;
pub mod runtime;
pub mod system;

pub mod singbox;
pub mod subscription;

// ---------------------------------------------------------------
// Backward-compatible flat re-exports.
//
// Keeping these means:
//   * external crates (`tauri-app`) keep using
//     `app_core::supervisor::Supervisor` as before;
//   * internal modules can keep `use crate::supervisor::...` style
//     imports — they resolve through these re-exports without
//     needing to know which group the leaf was sorted into.
//
// Adding a leaf? Put it in the right group, then add a `pub use`
// line below.
// ---------------------------------------------------------------

// model/
pub use model::{profile, types};

// runtime/
pub use runtime::{port_pick, process_guard, state_validator, supervisor, watchdog};

// net/
pub use net::{clash_api, network_repair, outbound_health, probe, wintun_cleanup};

// system/
pub use system::{elevation, hwid, power, state_guard, sys_proxy};

pub use error::{CoreError, CoreResult};
