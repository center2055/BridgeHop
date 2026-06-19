//! Pluggable-transport launcher for deep verification.
//!
//! Runs a PT client as a Tor "managed transport" (the standard PT spec handshake over env vars +
//! stdout), reads the `CMETHOD` line announcing its local SOCKS proxy, then performs a SOCKS5
//! CONNECT through it to the bridge's real endpoint — proving an actual transport handshake, not
//! just TCP reachability.
//!
//! This first version verifies **obfs4** (via `lyrebird` or `obfs4proxy`), the most common direct
//! transport. Binaries are located in BridgeHop's `pt` directory or an existing Tor Browser
//! install; when none is found the result degrades gracefully to "not installed".
//!
//! NOTE: the live handshake path requires a real PT binary and a reachable bridge, so it is
//! validated outside CI. The protocol pieces (CMETHOD parsing, SOCKS arg encoding) are unit-tested.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::socks::socks5_connect;
use crate::model::{Bridge, DeepResult, Transport};
use crate::paths;

/// Perform deep verification of a single bridge.
pub async fn deep_verify(
    bridge: &Bridge,
    timeout: Duration,
    cancel: &CancellationToken,
) -> DeepResult {
    let method = bridge.transport.token().to_string();

    if !matches!(bridge.transport, Transport::Obfs4) {
        return fail(
            &method,
            format!("deep verify not yet supported for {method}"),
        );
    }
    let Some(endpoint) = &bridge.endpoint else {
        return fail(&method, "no endpoint to verify".to_string());
    };
    let Some(binary) = locate(&["lyrebird", "obfs4proxy"]) else {
        return fail(
            &method,
            "obfs4 client (lyrebird/obfs4proxy) not installed — install it or Tor Browser"
                .to_string(),
        );
    };

    match run_obfs4(
        &binary,
        bridge,
        &endpoint.host,
        endpoint.port,
        timeout,
        cancel,
    )
    .await
    {
        Ok(ms) => DeepResult {
            ok: true,
            method,
            socks_ms: Some(ms),
            detail: format!("obfs4 handshake ok in {ms} ms"),
        },
        Err(detail) => fail(&method, detail),
    }
}

fn fail(method: &str, detail: String) -> DeepResult {
    DeepResult {
        ok: false,
        method: method.to_string(),
        socks_ms: None,
        detail,
    }
}

async fn run_obfs4(
    binary: &PathBuf,
    bridge: &Bridge,
    host: &str,
    port: u16,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<u32, String> {
    let state = std::env::temp_dir().join(format!("bridgehop-pt-{}", bridge.id));
    let _ = std::fs::create_dir_all(&state);

    let mut child = Command::new(binary)
        .env("TOR_PT_MANAGED_TRANSPORT_VER", "1")
        .env("TOR_PT_CLIENT_TRANSPORTS", "obfs4")
        .env("TOR_PT_STATE_LOCATION", &state)
        .env("TOR_PT_EXIT_ON_STDIN_CLOSE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not start PT client: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "PT client produced no stdout".to_string())?;

    let outcome = async {
        let socks = read_cmethod(stdout, "obfs4", timeout, cancel).await?;
        let (user, pass) = obfs4_socks_args(bridge);
        socks5_connect(socks, host, port, Some((&user, &pass)), timeout, cancel).await
    }
    .await;

    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&state);
    outcome
}

/// Read the PT's stdout until it announces a SOCKS proxy for `want`, or errors / times out.
async fn read_cmethod(
    stdout: tokio::process::ChildStdout,
    want: &str,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<SocketAddr, String> {
    let mut lines = BufReader::new(stdout).lines();
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err("cancelled".to_string()),
            _ = &mut deadline => return Err("PT client did not report a SOCKS proxy in time".to_string()),
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some(addr) = parse_cmethod(&line, want) {
                        return Ok(addr);
                    }
                    if line.starts_with("CMETHOD-ERROR")
                        || line.starts_with("ENV-ERROR")
                        || line.starts_with("VERSION-ERROR")
                    {
                        return Err(line);
                    }
                    if line.starts_with("CMETHODS DONE") {
                        return Err(format!("PT client did not provide {want}"));
                    }
                }
                Ok(None) => return Err("PT client exited before announcing a proxy".to_string()),
                Err(e) => return Err(format!("reading PT client output: {e}")),
            }
        }
    }
}

/// Parse a `CMETHOD <transport> socks5 <addr>` line, returning the SOCKS address for `want`.
fn parse_cmethod(line: &str, want: &str) -> Option<SocketAddr> {
    let mut it = line.split_whitespace();
    if it.next()? != "CMETHOD" {
        return None;
    }
    if it.next()? != want {
        return None;
    }
    if it.next()? != "socks5" {
        return None;
    }
    it.next()?.parse().ok()
}

/// Build the SOCKS5 username/password carrying obfs4's per-bridge arguments (cert, iat-mode).
fn obfs4_socks_args(bridge: &Bridge) -> (String, String) {
    let cert = bridge.params.get("cert").cloned().unwrap_or_default();
    let iat = bridge
        .params
        .get("iat-mode")
        .cloned()
        .unwrap_or_else(|| "0".to_string());
    split_socks_args(&format!("cert={cert};iat-mode={iat}"))
}

/// Split a PT args string across SOCKS5 username/password (each ≤255 bytes), per the PT spec.
fn split_socks_args(args: &str) -> (String, String) {
    if args.len() <= 255 {
        // A non-empty password is required by RFC 1929; a single NUL is the conventional filler.
        (args.to_string(), "\0".to_string())
    } else {
        let (head, tail) = args.split_at(255);
        (head.to_string(), tail.to_string())
    }
}

/// Whether an obfs4 client (lyrebird or obfs4proxy) is installed and locatable.
pub fn obfs4_available() -> bool {
    locate(&["lyrebird", "obfs4proxy"]).is_some()
}

/// The directory BridgeHop looks in for pluggable-transport binaries.
pub fn pt_dir() -> PathBuf {
    paths::data_dir().join("pt")
}

/// The platform executable name for a PT base name.
fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Find the first existing PT binary among `names`, searching BridgeHop's `pt` dir then Tor Browser.
fn locate(names: &[&str]) -> Option<PathBuf> {
    let mut dirs = vec![pt_dir()];
    dirs.extend(tor_browser_pt_dirs());
    for dir in dirs {
        for name in names {
            let candidate = dir.join(exe_name(name));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Best-effort Tor Browser PluggableTransports directories per OS.
fn tor_browser_pt_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());

    if cfg!(target_os = "windows") {
        if let Some(home) = &home {
            dirs.push(home.join(r"Desktop\Tor Browser\Browser\TorBrowser\Tor\PluggableTransports"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from(
            "/Applications/Tor Browser.app/Contents/MacOS/Tor/PluggableTransports",
        ));
        if let Some(home) = &home {
            dirs.push(
                home.join("Applications/Tor Browser.app/Contents/MacOS/Tor/PluggableTransports"),
            );
        }
    } else if let Some(home) = &home {
        dirs.push(home.join(
            ".local/share/torbrowser/tbb/x86_64/tor-browser/Browser/TorBrowser/Tor/PluggableTransports",
        ));
        dirs.push(home.join("Desktop/Tor Browser/Browser/TorBrowser/Tor/PluggableTransports"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_bridge_line;

    #[test]
    fn parses_cmethod_line() {
        let addr = parse_cmethod("CMETHOD obfs4 socks5 127.0.0.1:41000", "obfs4").unwrap();
        assert_eq!(addr.port(), 41000);
        assert!(parse_cmethod("CMETHOD obfs4 socks5 127.0.0.1:41000", "snowflake").is_none());
        assert!(parse_cmethod("CMETHODS DONE", "obfs4").is_none());
    }

    #[test]
    fn obfs4_args_encode_cert_and_iat() {
        let bridge = parse_bridge_line(
            "obfs4 5.6.7.8:1234 A7E7616C91B2FD83005B986A816EE9365F1360F4 cert=ABCDEF iat-mode=1",
        )
        .unwrap();
        let (user, pass) = obfs4_socks_args(&bridge);
        assert_eq!(user, "cert=ABCDEF;iat-mode=1");
        assert_eq!(pass, "\0");
    }

    #[test]
    fn long_args_split_across_fields() {
        let long = "cert=".to_string() + &"A".repeat(300);
        let (user, pass) = split_socks_args(&long);
        assert_eq!(user.len(), 255);
        assert_eq!(pass.len(), long.len() - 255);
    }
}
