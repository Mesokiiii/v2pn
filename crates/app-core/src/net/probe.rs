//! TCP-handshake latency probe.
//!
//! We measure the time to complete a TCP three-way handshake to the proxy
//! server's `host:port`. This is what every modern client (Happ, v2rayN,
//! sing-box, Clash) uses as the headline RTT — it correlates well with
//! actual user-perceived latency without leaking the inner protocol.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);
const MAX_CONCURRENCY: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub profile_id: String,
    /// Round-trip time in milliseconds, or `None` if the host was
    /// unreachable / timed out.
    pub rtt_ms: Option<u32>,
}

/// Single probe — TCP connect, return elapsed time on success.
pub async fn tcp_ping(host: &str, port: u16) -> Option<Duration> {
    let target = format!("{host}:{port}");
    let started = Instant::now();
    let result = tokio::time::timeout(PROBE_TIMEOUT, TcpStream::connect(&target)).await;
    match result {
        Ok(Ok(stream)) => {
            // Drop immediately so the OS sends RST and the server doesn't
            // log a half-open connection.
            drop(stream);
            Some(started.elapsed())
        }
        Ok(Err(_)) | Err(_) => None,
    }
}

/// Probe many profiles in parallel with a concurrency cap. Order of the
/// returned vector matches the order of the input.
pub async fn probe_many(targets: Vec<(String, String, u16)>) -> Vec<PingResult> {
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENCY));
    let mut handles = Vec::with_capacity(targets.len());

    for (id, host, port) in targets {
        let permit = sem.clone();
        handles.push(tokio::spawn(async move {
            let _p = permit.acquire_owned().await.ok();
            let rtt = tcp_ping(&host, port).await.map(|d| d.as_millis() as u32);
            PingResult { profile_id: id, rtt_ms: rtt }
        }));
    }

    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(r) = h.await {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unreachable_host_times_out() {
        // RFC 5737 TEST-NET — unroutable.
        let r = tcp_ping("192.0.2.1", 65535).await;
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn loopback_responds_quickly_or_refuses() {
        // We can't guarantee a listener exists, but the call must finish
        // (either Some or None) within PROBE_TIMEOUT.
        let _ = tcp_ping("127.0.0.1", 1).await;
    }
}
