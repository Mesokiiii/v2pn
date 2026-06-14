//! Stub system proxy implementation for non-Windows platforms.
//!
//! Returns a default snapshot and silently no-ops apply/restore. The Windows
//! build supplies the real implementation in [`super::windows`].

use crate::sys_proxy::{ProxySnapshot, SystemProxy};
use crate::CoreResult;

#[derive(Debug, Default, Clone)]
pub struct StubSystemProxy;

impl StubSystemProxy {
    pub fn new() -> Self {
        Self
    }
}

impl SystemProxy for StubSystemProxy {
    fn snapshot(&self) -> CoreResult<ProxySnapshot> {
        Ok(ProxySnapshot::default())
    }
    fn apply(&self, _addr: &str, _bypass: &[&str]) -> CoreResult<()> {
        Ok(())
    }
    fn restore(&self, _snap: &ProxySnapshot) -> CoreResult<()> {
        Ok(())
    }
}
