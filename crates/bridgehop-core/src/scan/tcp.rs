//! Tier T1: low-level TCP reachability probe.
//!
//! Ported from OnionHop's `BridgeScanService.ProbeTcpAsync` / `DescribeSocketError`: open a TCP
//! connection to `host:port` (resolving DNS for hostnames) and time the handshake.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use tokio::net::{lookup_host, TcpStream};
use tokio_util::sync::CancellationToken;

/// Open a TCP connection and time the handshake.
///
/// Returns elapsed milliseconds on success, or a short error description on failure.
pub(crate) async fn probe_tcp(
    host: &str,
    port: u16,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<u32, String> {
    let addr = resolve(host, port, timeout, cancel).await?;
    let start = Instant::now();
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("cancelled".to_string()),
        result = tokio::time::timeout(timeout, TcpStream::connect(addr)) => match result {
            Ok(Ok(_stream)) => Ok(start.elapsed().as_millis() as u32),
            Ok(Err(err)) => Err(describe_io_error(&err)),
            Err(_) => Err("timed out".to_string()),
        }
    }
}

/// Resolve `host:port` to a single socket address, skipping DNS for IP literals.
pub(crate) async fn resolve(
    host: &str,
    port: u16,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<SocketAddr, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("cancelled".to_string()),
        result = tokio::time::timeout(timeout, lookup_host((host, port))) => match result {
            Ok(Ok(mut addrs)) => addrs.next().ok_or_else(|| "DNS: no address".to_string()),
            Ok(Err(_)) => Err("DNS failed".to_string()),
            Err(_) => Err("DNS timed out".to_string()),
        }
    }
}

/// Map an I/O error to a short, user-facing detail string (mirrors `DescribeSocketError`).
pub(crate) fn describe_io_error(err: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::ConnectionRefused => "refused".to_string(),
        ErrorKind::TimedOut => "timed out".to_string(),
        ErrorKind::HostUnreachable => "host unreachable".to_string(),
        ErrorKind::NetworkUnreachable => "network unreachable".to_string(),
        _ => err.to_string(),
    }
}
