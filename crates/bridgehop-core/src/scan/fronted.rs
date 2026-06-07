//! Tier T2: front-host probe for fronted / broker transports.
//!
//! OnionHop probes the broker/front host on `:443` with a plain TCP connect. BridgeHop keeps
//! that reachability signal and adds a best-effort TLS handshake: a completed handshake is
//! stronger evidence that domain fronting actually works from this network. The TLS step never
//! causes a false negative — as long as the TCP connect succeeds the bridge reports as `Fronted`,
//! and the handshake outcome is surfaced only in the detail string.

use std::sync::Arc;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use tokio::net::TcpStream;
use tokio_rustls::rustls::crypto::ring::default_provider;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

use super::tcp::{describe_io_error, resolve};

/// Shared client TLS config trusting the webpki root set, built with the ring crypto provider.
static TLS_CONFIG: Lazy<Arc<ClientConfig>> = Lazy::new(|| {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
});

/// Probe a front/broker host. On a successful TCP connect returns `(tcp_ms, tls_ok)`; otherwise
/// a short error description.
pub(crate) async fn probe_front(
    host: &str,
    port: u16,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<(u32, bool), String> {
    let addr = resolve(host, port, timeout, cancel).await?;
    let start = Instant::now();
    let stream = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("cancelled".to_string()),
        result = tokio::time::timeout(timeout, TcpStream::connect(addr)) => match result {
            Ok(Ok(stream)) => stream,
            Ok(Err(err)) => return Err(describe_io_error(&err)),
            Err(_) => return Err("timed out".to_string()),
        }
    };
    let tcp_ms = start.elapsed().as_millis() as u32;
    let tls_ok = try_tls_handshake(stream, host, timeout, cancel).await;
    Ok((tcp_ms, tls_ok))
}

/// Best-effort TLS handshake to `host` over an established stream. Never panics; returns whether
/// the handshake completed.
async fn try_tls_handshake(
    stream: TcpStream,
    host: &str,
    timeout: Duration,
    cancel: &CancellationToken,
) -> bool {
    let Ok(server_name) = ServerName::try_from(host.to_string()) else {
        return false;
    };
    let connector = TlsConnector::from(Arc::clone(&TLS_CONFIG));
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        result = tokio::time::timeout(timeout, connector.connect(server_name, stream)) => {
            matches!(result, Ok(Ok(_)))
        }
    }
}
