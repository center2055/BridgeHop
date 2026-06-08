//! Minimal SOCKS5 client (RFC 1928 + RFC 1929 username/password auth).
//!
//! Used by deep verification to connect through a pluggable transport's local SOCKS proxy to the
//! bridge's real endpoint. Pluggable transports receive per-bridge arguments (e.g. obfs4 `cert`
//! and `iat-mode`) via the SOCKS username/password fields, per the Tor PT spec.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

/// Perform a SOCKS5 CONNECT to `target_host:target_port` through `proxy`, optionally authenticating.
/// Returns the elapsed milliseconds on a successful CONNECT reply.
pub(crate) async fn socks5_connect(
    proxy: SocketAddr,
    target_host: &str,
    target_port: u16,
    auth: Option<(&str, &str)>,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<u32, String> {
    let start = Instant::now();
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("cancelled".to_string()),
        result = tokio::time::timeout(timeout, handshake(proxy, target_host, target_port, auth)) => {
            match result {
                Ok(Ok(())) => Ok(start.elapsed().as_millis() as u32),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("timed out".to_string()),
            }
        }
    }
}

async fn handshake(
    proxy: SocketAddr,
    target_host: &str,
    target_port: u16,
    auth: Option<(&str, &str)>,
) -> Result<(), String> {
    let mut stream = TcpStream::connect(proxy)
        .await
        .map_err(|e| format!("connect proxy: {e}"))?;

    // Method negotiation.
    if auth.is_some() {
        stream
            .write_all(&[0x05, 0x02, 0x00, 0x02])
            .await
            .map_err(io)?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00]).await.map_err(io)?;
    }
    let mut selection = [0u8; 2];
    stream.read_exact(&mut selection).await.map_err(io)?;
    if selection[0] != 0x05 {
        return Err("not a SOCKS5 proxy".to_string());
    }
    match selection[1] {
        0x00 => {}
        0x02 => {
            let (user, pass) = auth.ok_or("proxy requested auth but none was provided")?;
            let mut msg = Vec::with_capacity(3 + user.len() + pass.len());
            msg.push(0x01);
            msg.push(clamp_len(user.len())?);
            msg.extend_from_slice(user.as_bytes());
            msg.push(clamp_len(pass.len())?);
            msg.extend_from_slice(pass.as_bytes());
            stream.write_all(&msg).await.map_err(io)?;
            let mut resp = [0u8; 2];
            stream.read_exact(&mut resp).await.map_err(io)?;
            if resp[1] != 0x00 {
                return Err("SOCKS authentication rejected".to_string());
            }
        }
        0xFF => return Err("no acceptable SOCKS auth method".to_string()),
        other => return Err(format!("unexpected SOCKS method {other:#x}")),
    }

    // CONNECT request.
    let mut req = vec![0x05, 0x01, 0x00];
    if let Ok(v4) = target_host.parse::<Ipv4Addr>() {
        req.push(0x01);
        req.extend_from_slice(&v4.octets());
    } else if let Ok(v6) = target_host.parse::<Ipv6Addr>() {
        req.push(0x04);
        req.extend_from_slice(&v6.octets());
    } else {
        let host = target_host.as_bytes();
        req.push(0x03);
        req.push(clamp_len(host.len())?);
        req.extend_from_slice(host);
    }
    req.extend_from_slice(&target_port.to_be_bytes());
    stream.write_all(&req).await.map_err(io)?;

    // Reply.
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await.map_err(io)?;
    if head[0] != 0x05 {
        return Err("bad SOCKS reply".to_string());
    }
    if head[1] != 0x00 {
        return Err(format!("SOCKS CONNECT failed (reply {:#x})", head[1]));
    }
    // Drain the bound address so the stream is left clean.
    let bound = match head[3] {
        0x01 => 4 + 2,
        0x04 => 16 + 2,
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await.map_err(io)?;
            len[0] as usize + 2
        }
        other => return Err(format!("bad SOCKS address type {other:#x}")),
    };
    let mut rest = vec![0u8; bound];
    stream.read_exact(&mut rest).await.map_err(io)?;
    Ok(())
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

fn clamp_len(len: usize) -> Result<u8, String> {
    u8::try_from(len).map_err(|_| "SOCKS field exceeds 255 bytes".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A tiny SOCKS5 server that accepts the negotiation and replies success to CONNECT.
    async fn mock_server(want_auth: bool) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            // greeting
            let mut g = [0u8; 2];
            s.read_exact(&mut g).await.unwrap();
            let mut methods = vec![0u8; g[1] as usize];
            s.read_exact(&mut methods).await.unwrap();
            if want_auth {
                s.write_all(&[0x05, 0x02]).await.unwrap();
                // read auth: VER, ULEN, user, PLEN, pass
                let mut h = [0u8; 2];
                s.read_exact(&mut h).await.unwrap();
                let mut user = vec![0u8; h[1] as usize];
                s.read_exact(&mut user).await.unwrap();
                let mut pl = [0u8; 1];
                s.read_exact(&mut pl).await.unwrap();
                let mut pass = vec![0u8; pl[0] as usize];
                s.read_exact(&mut pass).await.unwrap();
                s.write_all(&[0x01, 0x00]).await.unwrap();
            } else {
                s.write_all(&[0x05, 0x00]).await.unwrap();
            }
            // connect request: VER, CMD, RSV, ATYP, ...
            let mut head = [0u8; 4];
            s.read_exact(&mut head).await.unwrap();
            let addr_len = match head[3] {
                0x01 => 4,
                0x04 => 16,
                0x03 => {
                    let mut l = [0u8; 1];
                    s.read_exact(&mut l).await.unwrap();
                    l[0] as usize
                }
                _ => 0,
            };
            let mut rest = vec![0u8; addr_len + 2];
            s.read_exact(&mut rest).await.unwrap();
            // success reply, bound 0.0.0.0:0
            s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn connect_no_auth() {
        let proxy = mock_server(false).await;
        let cancel = CancellationToken::new();
        let ms = socks5_connect(proxy, "1.2.3.4", 443, None, Duration::from_secs(2), &cancel)
            .await
            .unwrap();
        let _ = ms;
    }

    #[tokio::test]
    async fn connect_with_auth_and_domain() {
        let proxy = mock_server(true).await;
        let cancel = CancellationToken::new();
        socks5_connect(
            proxy,
            "bridge.example.net",
            443,
            Some(("cert=abc;iat-mode=0", "x")),
            Duration::from_secs(2),
            &cancel,
        )
        .await
        .unwrap();
    }
}
