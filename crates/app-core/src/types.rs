//! Type-level safety wrappers around small string identifiers that v2pn
//! passes through public APIs. The goal isn't to *forbid* misuse — it's to
//! make accidents loud at compile time.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Validated name of a Windows TUN interface. Constraints:
/// `1..=64` chars, ASCII alphanumerics + `-` + `_`. This is what Wintun's
/// `WintunCreateAdapter` accepts, and conveniently also what sing-box's
/// `interface_name` field tolerates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TunInterfaceName(String);

#[derive(Debug, Clone, thiserror::Error)]
pub enum TunNameError {
    #[error("interface name must be 1..=64 chars")]
    Length,
    #[error("interface name must be ASCII alphanumeric, '-' or '_'")]
    Charset,
}

impl TunInterfaceName {
    pub fn new(s: impl Into<String>) -> Result<Self, TunNameError> {
        let s = s.into();
        if s.is_empty() || s.len() > 64 {
            return Err(TunNameError::Length);
        }
        if !s
            .as_bytes()
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(TunNameError::Charset);
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TunInterfaceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Default for TunInterfaceName {
    fn default() -> Self {
        // SAFETY (-not-really-unsafe): the literal satisfies the validator.
        Self::new("v2pn-tun").expect("static literal is valid")
    }
}

/// A loopback-only TCP port number. We use this where the public API of v2pn
/// would otherwise accept any `u16`, so callers can't accidentally hand us
/// a privileged port (1..=1023) or 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LoopbackPort(u16);

#[derive(Debug, Clone, thiserror::Error)]
pub enum LoopbackPortError {
    #[error("port must be 1024..=65535, got {0}")]
    OutOfRange(u16),
}

impl LoopbackPort {
    pub const fn new_unchecked(p: u16) -> Self {
        Self(p)
    }

    pub fn new(p: u16) -> Result<Self, LoopbackPortError> {
        if p < 1024 {
            return Err(LoopbackPortError::OutOfRange(p));
        }
        Ok(Self(p))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tun_name_rejects_garbage() {
        assert!(TunInterfaceName::new("ok-name_1").is_ok());
        assert!(TunInterfaceName::new("").is_err());
        assert!(TunInterfaceName::new("with space").is_err());
        assert!(TunInterfaceName::new("спб-tun").is_err());
        assert!(TunInterfaceName::new("a".repeat(65)).is_err());
    }

    #[test]
    fn loopback_port_rejects_privileged() {
        assert!(LoopbackPort::new(80).is_err());
        assert!(LoopbackPort::new(1023).is_err());
        assert!(LoopbackPort::new(1024).is_ok());
        assert!(LoopbackPort::new(65535).is_ok());
    }
}
