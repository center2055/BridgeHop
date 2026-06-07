//! Reachability scanning engine: a tiered probe pipeline with bounded concurrency,
//! cancellation, and streaming progress.
//!
//! The pipeline mirrors OnionHop's `BridgeScanService.ProbeAsync`:
//!
//! 1. Fronted transports — or any line whose only endpoint is a documentation placeholder —
//!    are probed at their broker/front host on `:443` (tier T2, [`fronted`]).
//! 2. Lines with no usable endpoint are reported as [`Reachability::Unparsed`].
//! 3. Everything else gets a direct TCP probe (tier T1, [`tcp`]).
//!
//! Tier T3 (deep pluggable-transport verification) lands in a later phase behind the
//! `deep-verify` feature.

mod fronted;
mod tcp;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::model::{Bridge, Reachability, ScanResult, SLOW_THRESHOLD_MS};
use crate::parse::is_placeholder_host;

const MIN_WORKERS: usize = 1;
const MAX_WORKERS: usize = 64;
const MIN_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_TIMEOUT: Duration = Duration::from_secs(60);

/// Options controlling a scan run.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Maximum concurrent probes (clamped to 1..=64).
    pub workers: usize,
    /// Per-probe timeout (clamped to 0.5s..=60s).
    pub timeout: Duration,
    /// Request deep (real pluggable-transport handshake) verification where supported.
    /// Honored only when the `deep-verify` feature is enabled and on desktop.
    pub deep: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            workers: 16,
            timeout: Duration::from_secs(3),
            deep: false,
        }
    }
}

/// Scan a batch of parsed bridges, streaming each [`ScanResult`] over `progress` as it completes
/// and returning the full set when finished.
///
/// Concurrency is bounded by `options.workers`; cancelling `cancel` stops further probes promptly
/// and aborts in-flight connects. A dropped `progress` receiver is not an error — the scan still
/// runs to completion and returns its results.
pub async fn scan(
    bridges: Vec<Bridge>,
    options: ScanOptions,
    progress: mpsc::Sender<ScanResult>,
    cancel: CancellationToken,
) -> Vec<ScanResult> {
    let workers = options.workers.clamp(MIN_WORKERS, MAX_WORKERS);
    let timeout = options.timeout.clamp(MIN_TIMEOUT, MAX_TIMEOUT);
    let semaphore = Arc::new(Semaphore::new(workers));

    let mut handles = Vec::with_capacity(bridges.len());
    for bridge in bridges {
        let semaphore = Arc::clone(&semaphore);
        let progress = progress.clone();
        let cancel = cancel.clone();
        handles.push(tokio::spawn(async move {
            // The semaphore is never closed, so acquisition only fails if the Arc is dropped.
            let _permit = semaphore.acquire_owned().await.ok()?;
            if cancel.is_cancelled() {
                return None;
            }
            let result = probe_bridge(&bridge, timeout, &cancel).await;
            let _ = progress.send(result.clone()).await;
            Some(result)
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        if let Ok(Some(result)) = handle.await {
            results.push(result);
        }
    }
    results
}

/// Probe a single bridge through the tiered pipeline.
pub async fn probe_bridge(
    bridge: &Bridge,
    timeout: Duration,
    cancel: &CancellationToken,
) -> ScanResult {
    let placeholder_endpoint = bridge
        .endpoint
        .as_ref()
        .is_some_and(|e| is_placeholder_host(&e.host));

    if bridge.transport.is_fronted() || placeholder_endpoint {
        return probe_fronted(bridge, timeout, cancel).await;
    }

    match &bridge.endpoint {
        None => make_result(bridge, "?", 0, None, Reachability::Unparsed, "no host:port"),
        Some(ep) => match tcp::probe_tcp(&ep.host, ep.port, timeout, cancel).await {
            Ok(ms) => {
                let reachability = if u64::from(ms) < SLOW_THRESHOLD_MS {
                    Reachability::Reachable
                } else {
                    Reachability::Slow
                };
                make_result(
                    bridge,
                    &ep.host,
                    ep.port,
                    Some(ms),
                    reachability,
                    &format!("{ms} ms"),
                )
            }
            Err(detail) => make_result(
                bridge,
                &ep.host,
                ep.port,
                None,
                Reachability::Unreachable,
                &detail,
            ),
        },
    }
}

async fn probe_fronted(
    bridge: &Bridge,
    timeout: Duration,
    cancel: &CancellationToken,
) -> ScanResult {
    let Some(front) = bridge.front_host.as_deref() else {
        return make_result(
            bridge,
            "(fronted)",
            0,
            None,
            Reachability::Unparsed,
            "no broker/front host",
        );
    };

    match fronted::probe_front(front, 443, timeout, cancel).await {
        Ok((ms, tls_ok)) => {
            let detail = if tls_ok {
                format!("{ms} ms")
            } else {
                format!("{ms} ms (no TLS)")
            };
            make_result(bridge, front, 443, Some(ms), Reachability::Fronted, &detail)
        }
        Err(detail) => make_result(bridge, front, 443, None, Reachability::Unreachable, &detail),
    }
}

fn make_result(
    bridge: &Bridge,
    probed_host: &str,
    probed_port: u16,
    ping_ms: Option<u32>,
    reachability: Reachability,
    detail: &str,
) -> ScanResult {
    ScanResult {
        bridge_id: bridge.id.clone(),
        raw: bridge.raw.clone(),
        transport: bridge.transport.clone(),
        probed_host: probed_host.to_string(),
        probed_port,
        ping_ms,
        reachability,
        detail: detail.to_string(),
        deep: None,
        geo: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_bridge_line;
    use tokio::net::TcpListener;

    const FP: &str = "A7E7616C91B2FD83005B986A816EE9365F1360F4";

    #[tokio::test]
    async fn reachable_local_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let bridge = parse_bridge_line(&format!("127.0.0.1:{port} {FP}")).unwrap();

        let cancel = CancellationToken::new();
        let res = probe_bridge(&bridge, Duration::from_secs(2), &cancel).await;

        assert!(matches!(
            res.reachability,
            Reachability::Reachable | Reachability::Slow
        ));
        assert!(res.ping_ms.is_some());
        assert!(res.is_working());
    }

    #[tokio::test]
    async fn unreachable_closed_port() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener); // nothing listens here now

        let bridge = parse_bridge_line(&format!("127.0.0.1:{port} {FP}")).unwrap();
        let cancel = CancellationToken::new();
        let res = probe_bridge(&bridge, Duration::from_secs(2), &cancel).await;

        assert_eq!(res.reachability, Reachability::Unreachable);
        assert!(res.ping_ms.is_none());
    }

    #[tokio::test]
    async fn fronted_without_front_host_is_unparsed() {
        let bridge = parse_bridge_line(&format!("snowflake 192.0.2.3:80 {FP}")).unwrap();
        let cancel = CancellationToken::new();
        let res = probe_bridge(&bridge, Duration::from_millis(500), &cancel).await;

        assert_eq!(res.reachability, Reachability::Unparsed);
        assert_eq!(res.detail, "no broker/front host");
    }

    #[tokio::test]
    async fn front_probe_reports_tcp_success_without_tls() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept then immediately close so the TLS handshake fails fast (EOF).
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });

        let cancel = CancellationToken::new();
        let (_, tls_ok) = fronted::probe_front("127.0.0.1", port, Duration::from_secs(2), &cancel)
            .await
            .unwrap();
        assert!(!tls_ok);
    }

    #[tokio::test]
    async fn scan_streams_and_returns_results() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let bridges = vec![parse_bridge_line(&format!("127.0.0.1:{port} {FP}")).unwrap()];

        let (tx, mut rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let results = scan(
            bridges,
            ScanOptions {
                workers: 4,
                timeout: Duration::from_secs(2),
                deep: false,
            },
            tx,
            cancel,
        )
        .await;

        assert_eq!(results.len(), 1);
        let streamed = rx.recv().await.unwrap();
        assert_eq!(streamed.reachability, results[0].reachability);
    }

    #[tokio::test]
    async fn cancelled_scan_returns_no_results() {
        let bridges = vec![parse_bridge_line(&format!("10.255.255.1:9 {FP}")).unwrap()];
        let (tx, _rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        cancel.cancel(); // cancel before starting

        let results = scan(bridges, ScanOptions::default(), tx, cancel).await;
        assert!(results.is_empty());
    }
}
