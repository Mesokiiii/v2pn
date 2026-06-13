//! v2pn core: business logic with no UI dependencies.

pub mod error;
pub mod hwid;
pub mod elevation;
pub mod subscription;
pub mod profile;
pub mod singbox;
pub mod supervisor;
pub mod sys_proxy;
pub mod state_guard;
pub mod watchdog;
pub mod state_validator;
pub mod outbound_health;
pub mod process_guard;
pub mod port_pick;
pub mod wintun_cleanup;
pub mod power;
pub mod probe;
pub mod types;

pub use error::{CoreError, CoreResult};
