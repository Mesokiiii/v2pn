//! Pick a free local TCP port, falling back from a preferred number.
//!
//! Why this exists: the connect path needs `mixed_port` (7890) and
//! `clash_api_port` (9090) to be free *for sing-box*. If anything else on
//! the box is already listening — a stale orphan we couldn't kill, a
//! corporate proxy, another Clash-derived client — sing-box would refuse
//! to start with a cryptic `bind: Only one usage of each socket address
//! is normally permitted` error in the log, leaving the user staring at
//! `Failed`. We prevent that by walking a short list of candidate ports
//! starting at the user's preferred number, returning the first one we
//! could bind ourselves and immediately release. Sing-box gets to grab it
//! on the next cycle.
//!
//! There is a tiny TOCTOU window between our test-bind and sing-box's
//! production bind. In practice it never matters because:
//!   * the candidate range is 7890..7899 / 9090..9099 — millions of
//!     other ports stay free for a casual race winner;
//!   * sing-box itself prints an actionable bind error if it loses the
//!     race, and our death-watcher / auto-restart loop pick it up.
//!
//! Returns the first port that bound successfully, or the original
//! preferred port if nothing in the range was free (the caller is then
//! free to surface the original "address in use" error to the user).

use std::net::{Ipv4Addr, SocketAddrV4};

/// How many alternates we try before giving up. 10 covers every realistic
/// collision (corporate clash + meta + this tool). More than that and the
/// user's box is so contended that sing-box probably can't run anyway.
const MAX_TRIES: u16 = 10;

/// Find a free port for a 127.0.0.1 listener, starting at `preferred`. The
/// returned port might equal `preferred` (best case) or be `preferred+N`
/// for some `N <= MAX_TRIES`. If nothing in the range was free we hand
/// back `preferred` so the caller's existing error path stays in charge.
pub fn pick_free_port(preferred: u16) -> u16 {
    for delta in 0..MAX_TRIES {
        let candidate = preferred.saturating_add(delta);
        if candidate == 0 {
            continue;
        }
        if is_loopback_port_free(candidate) {
            return candidate;
        }
        tracing::debug!(target: "port_pick",
            "127.0.0.1:{candidate} occupied, trying next");
    }
    tracing::warn!(target: "port_pick",
        "no free port found in [{preferred}, {}); falling back to {preferred}",
        preferred.saturating_add(MAX_TRIES));
    preferred
}

/// Probe whether a TCP port on 127.0.0.1 is bindable. We open a real
/// listener and immediately drop it — that's the only way to know without
/// false positives (a `connect()` to a non-listening port doesn't tell us
/// if a different process *would* be allowed to bind, only that nobody
/// is currently listening).
fn is_loopback_port_free(port: u16) -> bool {
    use std::net::TcpListener;
    // SO_EXCLUSIVEADDRUSE is the Windows default for fresh sockets; we
    // don't need to set it explicitly. On Unix the equivalent (no
    // SO_REUSEADDR) is the std default too.
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_returns_preferred_when_free() {
        // High-numbered port that is virtually never in use on a CI box.
        let port = pick_free_port(54321);
        assert!((54321..54331).contains(&port));
    }

    #[test]
    fn pick_walks_past_occupied_port() {
        // Squat on `base` so the picker has to advance.
        use std::net::TcpListener;
        let base = 53737;
        let _hold = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, base)).unwrap();
        let pick = pick_free_port(base);
        assert_ne!(pick, base, "picker must advance past held port");
        assert!(pick > base && pick < base + MAX_TRIES);
    }
}
