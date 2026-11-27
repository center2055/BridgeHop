#![forbid(unsafe_code)]

//! HopForge engine: one process behind every front end.
//!
//! It reads one JSON command per line on stdin and writes one JSON event per line on stdout.
//! The Android shell (`apps/android`), the desktop CLI and the end-to-end tests all drive this
//! same binary, so what is exercised on a desktop is byte-for-byte what runs on the phone.
//!
//! Commands: `version`, `status`, `probe`, `connect`, `stop`.
//! Events: `probe`, `plan`, `tier`, `progress`, `log`, `connected`, `failed`.
//!
//! `connect` runs the ladder as far as it can from where it runs: T1 (TCP) and T2 (TLS) are
//! measured by this process; T3 (transport handshake), T4 (bootstrap) and T5 (end to end) are
//! observed through tor. A bridge is only reported "connected" after T5 passes.

use std::fs;
use std::io::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use hf_core::{parse_lines, Bridge, ProbeTarget, Transport};
use serde_json::{json, Value};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Upper bound on bridges measured before a connect, so one button press never scans a feed.
const MAX_PROBED: usize = 80;
/// Concurrent probes per wave. Kept low on purpose: this is a rate limit (R11).
const PROBE_WAVE: usize = 12;
/// Pause between waves; every probe is also delayed by up to this much (jitter).
const WAVE_PAUSE_MS: u64 = 250;
/// Early stop: once this many fast obfs4 bridges are known, the rest of the list is left alone.
const FAST_OBFS4_NEEDED: usize = 6;
const FAST_OBFS4_MS: u128 = 400;

/// The running session. Dropping it kills tor, and tor's exit closes the transports' stdin,
/// which is how managed pluggable transports are told to quit.
struct Shared {
    tor: Arc<Mutex<Child>>,
    socks: u16,
    http: u16,
    started: Instant,
}

impl Drop for Shared {
    fn drop(&mut self) {
        if let Ok(mut tor) = self.tor.lock() {
            let _ = tor.kill();
            let _ = tor.wait();
        }
    }
}

static SESSION: Mutex<Option<Shared>> = Mutex::new(None);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("probe") {
        let timeout = Duration::from_millis(
            args.get(2)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(3_000),
        );
        let mut text = String::new();
        let _ = io::stdin().read_to_string(&mut text);
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        println!(
            "{}",
            json!({ "ok": true, "results": probe_all(&lines, timeout) })
        );
        return;
    }
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        emit(&handle(&line));
    }
    let _ = stop();
}

fn handle(line: &str) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => return json!({ "ok": false, "error": format!("bad json: {err}") }),
    };
    match req["cmd"].as_str().unwrap_or("") {
        "version" => json!({ "ok": true, "version": VERSION }),
        "status" => status().unwrap_or_else(|err| json!({ "ok": false, "error": err })),
        "stop" => stop().unwrap_or_else(|err| json!({ "ok": false, "error": err })),
        "probe" => {
            let timeout = Duration::from_millis(req["timeout_ms"].as_u64().unwrap_or(3_000));
            json!({ "ok": true, "results": probe_all(&string_list(&req["lines"]), timeout) })
        }
        "connect" => start_connect(&req).unwrap_or_else(|err| json!({ "ok": false, "error": err })),
        "" => json!({ "ok": false, "error": "missing cmd" }),
        other => json!({ "ok": false, "error": format!("unknown cmd {other}") }),
    }
}

/// One JSON object per line; the lock keeps concurrent events on separate lines.
fn emit(event: &Value) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{event}");
    let _ = out.flush();
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn status() -> Result<Value, String> {
    let guard = SESSION.lock().map_err(|_| "engine lock poisoned")?;
    Ok(match guard.as_ref() {
        Some(session) => json!({
            "ok": true,
            "running": true,
            "socks_port": session.socks,
            "http_port": session.http,
            "uptime_ms": session.started.elapsed().as_millis() as u64,
        }),
        None => json!({ "ok": true, "running": false }),
    })
}

fn stop() -> Result<Value, String> {
    let mut guard = SESSION.lock().map_err(|_| "engine lock poisoned")?;
    let was_running = guard.is_some();
    *guard = None;
    Ok(json!({ "ok": true, "stopped": was_running }))
}

struct Options {
    work_dir: PathBuf,
    pt_dir: PathBuf,
    tor_bin: PathBuf,
    timeout: Duration,
    probe_timeout: Duration,
    socks: Option<u16>,
    http: Option<u16>,
    check_host: String,
}

fn path_of(req: &Value, key: &str, default: Option<&str>) -> Result<PathBuf, String> {
    match req[key].as_str().or(default) {
        Some(value) => Ok(PathBuf::from(value)),
        None => Err(format!("connect: {key} missing")),
    }
}

fn start_connect(req: &Value) -> Result<Value, String> {
    let opts = Options {
        work_dir: path_of(req, "work_dir", None)?,
        pt_dir: path_of(req, "pt_dir", None)?,
        tor_bin: path_of(req, "tor_bin", None)?,
        timeout: Duration::from_secs(req["timeout_sec"].as_u64().unwrap_or(150)),
        probe_timeout: Duration::from_millis(req["timeout_ms"].as_u64().unwrap_or(3_000)),
        socks: req["socks_port"].as_u64().map(|p| p as u16),
        http: req["http_port"].as_u64().map(|p| p as u16),
        check_host: req["check_host"]
            .as_str()
            .unwrap_or("check.torproject.org")
            .to_string(),
    };
    let lines = string_list(&req["lines"]);
    if lines.is_empty() {
        return Err("connect: no bridge lines".into());
    }
    stop()?;
    thread::spawn(move || {
        if let Err(reason) = connect(&opts, &lines) {
            emit(&json!({ "event": "failed", "reason": reason }));
        }
    });
    Ok(json!({ "ok": true, "started": true }))
}

fn dialable(lines: &[String]) -> Vec<Bridge> {
    parse_lines(lines)
        .bridges
        .into_iter()
        .filter(|bridge| bridge.transport.is_dialable())
        .collect()
}

struct Measured {
    bridge: Bridge,
    target: String,
    rtt_ms: Option<u128>,
    ok: bool,
    detail: String,
}

impl Measured {
    fn json(&self) -> Value {
        json!({
            "event": "probe",
            "id": self.bridge.id(),
            "transport": self.bridge.transport.name(),
            "target": self.target,
            "rtt_ms": self.rtt_ms,
            "ok": self.ok,
            "detail": self.detail,
        })
    }

    fn speed(&self) -> u128 {
        self.rtt_ms.unwrap_or(u128::MAX)
    }
}

/// T1 and T2 for one bridge, as far as the transport allows them to be measured directly.
fn measure_bridge(bridge: &Bridge, timeout: Duration) -> Measured {
    let mut measured = Measured {
        bridge: bridge.clone(),
        target: String::new(),
        rtt_ms: None,
        ok: false,
        detail: String::new(),
    };
    let (addr, tls_name) = match bridge.probe_target() {
        None => {
            measured.detail = "not probed: reached through a front".into();
            return measured;
        }
        Some(ProbeTarget::Addr(addr)) => (addr, None),
        Some(ProbeTarget::Host {
            host,
            port,
            server_name,
        }) => {
            measured.target = format!("{host}:{port}");
            match (host.as_str(), port).to_socket_addrs() {
                Ok(found) => match found.min_by_key(|a| a.is_ipv6()) {
                    Some(addr) => (addr, Some(server_name)),
                    None => {
                        measured.detail = "dns: no address".into();
                        return measured;
                    }
                },
                Err(err) => {
                    measured.detail = format!("dns: {err}");
                    return measured;
                }
            }
        }
    };
    if measured.target.is_empty() {
        measured.target = addr.to_string();
    }
    let started = Instant::now();
    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(_) => measured.rtt_ms = Some(started.elapsed().as_millis()),
        Err(err) => {
            measured.detail = format!("T1: {err}");
            return measured;
        }
    }
    // T2 is meaningful only where a real certificate is presented (the webtunnel URL host);
    // vanilla tor links are authenticated by fingerprint, not by the web PKI.
    if let Some(name) = tls_name {
        if let Err(err) = tls_handshake(addr, &name, timeout) {
            measured.detail = format!("T2: {err}");
            return measured;
        }
    }
    measured.ok = true;
    measured
}

/// Probes in prioritized waves (the feed's own order), rate limited and jittered, and stops as
/// soon as `enough` says the list is good enough (R11: do not burn bridges).
fn measure_waves<'a>(
    bridges: &[&'a Bridge],
    timeout: Duration,
    enough: &dyn Fn(&[Measured]) -> bool,
) -> Vec<Measured> {
    let mut done: Vec<Measured> = Vec::new();
    for wave in bridges.chunks(PROBE_WAVE) {
        let results: Vec<Measured> = thread::scope(|scope| {
            let handles: Vec<_> = wave
                .iter()
                .map(|bridge| {
                    scope.spawn(move || {
                        jitter();
                        measure_bridge(bridge, timeout)
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });
        for result in &results {
            emit(&result.json());
        }
        done.extend(results);
        if enough(&done) {
            break;
        }
        thread::sleep(Duration::from_millis(WAVE_PAUSE_MS));
    }
    done
}

fn probe_all(lines: &[String], timeout: Duration) -> Vec<Value> {
    let bridges = dialable(lines);
    let direct: Vec<&Bridge> = bridges
        .iter()
        .filter(|bridge| bridge.transport.is_direct())
        .take(MAX_PROBED)
        .collect();
    measure_waves(&direct, timeout, &|_| false)
        .iter()
        .map(Measured::json)
        .collect()
}

/// A delay of 0..WAVE_PAUSE_MS ms from a per-process random seed: no two probes leave together.
fn jitter() {
    use std::hash::{BuildHasher, Hash, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    thread::current().id().hash(&mut hasher);
    Instant::now().hash(&mut hasher);
    thread::sleep(Duration::from_millis(hasher.finish() % WAVE_PAUSE_MS));
}

fn connect(opts: &Options, lines: &[String]) -> Result<(), String> {
    let started = Instant::now();
    let bridges = dialable(lines);
    let direct: Vec<&Bridge> = bridges
        .iter()
        .filter(|bridge| bridge.transport.is_direct())
        .take(MAX_PROBED)
        .collect();
    let measured = measure_waves(&direct, opts.probe_timeout, &|done| {
        done.iter()
            .filter(|m| {
                m.ok && m.bridge.transport == Transport::Obfs4 && m.speed() <= FAST_OBFS4_MS
            })
            .count()
            >= FAST_OBFS4_NEEDED
    });

    // What answered goes to tor fastest first, each transport under its own quota; fronted
    // transports cannot be pre-probed, so they follow in the feed's order.
    let mut ordered: Vec<Bridge> = Vec::new();
    let mut plan = serde_json::Map::new();
    for (transport, quota) in [
        (Transport::Obfs4, 24),
        (Transport::Vanilla, 4),
        (Transport::Webtunnel, 8),
    ] {
        let mut reach: Vec<(u128, Bridge)> = measured
            .iter()
            .filter(|m| m.ok && m.bridge.transport == transport)
            .map(|m| (m.speed(), m.bridge.clone()))
            .collect();
        reach.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id().cmp(&b.1.id())));
        reach.truncate(quota);
        plan.insert(transport.name().to_string(), json!(reach.len()));
        ordered.extend(reach.into_iter().map(|(_, bridge)| bridge));
    }
    for (transport, quota) in [
        (Transport::Snowflake, 3),
        (Transport::MeekLite, 3),
        (Transport::Conjure, 2),
    ] {
        let picked: Vec<Bridge> = bridges
            .iter()
            .filter(|bridge| bridge.transport == transport)
            .take(quota)
            .cloned()
            .collect();
        plan.insert(transport.name().to_string(), json!(picked.len()));
        ordered.extend(picked);
    }
    emit(&json!({
        "event": "plan",
        "probed": measured.len(),
        "reachable": measured.iter().filter(|m| m.ok).count(),
        "transports": Value::Object(plan),
    }));
    if ordered.is_empty() {
        return Err("no bridge answered the reachability probe (T1/T2)".into());
    }

    let data = opts.work_dir.join("data");
    fs::create_dir_all(&data).map_err(|err| format!("cannot create {}: {err}", data.display()))?;
    let socks = match opts.socks {
        Some(port) => port,
        None => free_port().ok_or("no free SOCKS port")?,
    };
    let http = match opts.http {
        Some(port) => port,
        None => free_port().ok_or("no free HTTP port")?,
    };
    let control = free_port().ok_or("no free control port")?;

    let mut torrc = String::new();
    let _ = writeln!(torrc, "DataDirectory {}", quote(&data));
    let _ = writeln!(torrc, "SocksPort 127.0.0.1:{socks}");
    let _ = writeln!(torrc, "HTTPTunnelPort 127.0.0.1:{http}");
    let _ = writeln!(torrc, "ControlPort 127.0.0.1:{control}");
    torrc.push_str("CookieAuthentication 1\nUseBridges 1\n");
    let _ = writeln!(
        torrc,
        "Log notice file {}",
        quote(&opts.work_dir.join("tor.log"))
    );
    for (transports, sidecar) in [
        ("obfs4,meek_lite", "lyrebird"),
        ("webtunnel", "webtunnel"),
        ("snowflake", "snowflake"),
        ("conjure", "conjure"),
    ] {
        if let Some(path) = sidecar_path(&opts.pt_dir, sidecar) {
            let _ = writeln!(
                torrc,
                "ClientTransportPlugin {transports} exec {}",
                quote(&path)
            );
        }
    }
    for bridge in &ordered {
        let _ = writeln!(torrc, "Bridge {}", bridge.tor_line());
    }
    let torrc_path = opts.work_dir.join("torrc");
    fs::write(&torrc_path, torrc).map_err(|err| format!("cannot write torrc: {err}"))?;

    let child = Command::new(&opts.tor_bin)
        .arg("-f")
        .arg(&torrc_path)
        .current_dir(&opts.work_dir)
        .env("HOME", &opts.work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("cannot start {}: {err}", opts.tor_bin.display()))?;
    let tor = Arc::new(Mutex::new(child));
    *SESSION.lock().map_err(|_| "engine lock poisoned")? = Some(Shared {
        tor: tor.clone(),
        socks,
        http,
        started,
    });

    let cookie = read_cookie(&data, &tor).ok_or_else(|| {
        format!(
            "tor did not open its control port; {}",
            log_tail(&opts.work_dir)
        )
    })?;
    let (tx, rx) = mpsc::channel::<Value>();
    let control_addr = SocketAddr::from(([127, 0, 0, 1], control));
    thread::spawn(move || watch_control(control_addr, cookie, tx));

    let deadline = started + opts.timeout;
    let (mut progress, mut t3, mut t4) = (0u8, false, false);
    let mut summary = String::new();
    let mut bootstrapped_at: Option<Instant> = None;
    loop {
        {
            let mut guard = tor.lock().map_err(|_| "tor lock poisoned")?;
            if let Some(code) = guard.try_wait().map_err(|err| format!("tor: {err}"))? {
                return Err(format!(
                    "tor exited with {code} at {progress}%; {}",
                    log_tail(&opts.work_dir)
                ));
            }
        }
        if !still_current(&tor) {
            return Err("stopped".into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out at {progress}% ({summary}); {}",
                log_tail(&opts.work_dir)
            ));
        }
        let event = match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                // Tor does not always announce its first circuit; give it a moment after 100%.
                if bootstrapped_at.is_some_and(|at| at.elapsed() > Duration::from_secs(4)) {
                    break;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("lost the control connection at {progress}%"));
            }
        };
        match event["event"].as_str() {
            Some("progress") => {
                let now = u8::try_from(event["progress"].as_u64().unwrap_or(0)).unwrap_or(0);
                let text = event["summary"].as_str().unwrap_or("").to_string();
                if now != progress || text != summary {
                    emit(&event);
                }
                progress = progress.max(now);
                summary = text;
                if !t3 && progress >= 15 {
                    t3 = true;
                    emit(&json!({ "event": "tier", "tier": "T3", "pass": true }));
                }
                if !t4 && progress >= 100 {
                    t4 = true;
                    bootstrapped_at = Some(Instant::now());
                    emit(&json!({ "event": "tier", "tier": "T4", "pass": true }));
                }
            }
            Some("circuit") => break,
            Some("log") => emit(&event),
            Some("control_lost") => {
                return Err(format!("lost the control connection at {progress}%"))
            }
            _ => {}
        }
    }

    let socks_addr = SocketAddr::from(([127, 0, 0, 1], socks));
    match tor_check(socks_addr, &opts.check_host, Duration::from_secs(45)) {
        Ok(true) => {
            emit(&json!({ "event": "tier", "tier": "T5", "pass": true }));
            emit(&json!({
                "event": "connected",
                "socks_port": socks,
                "http_port": http,
                "elapsed_ms": started.elapsed().as_millis() as u64,
                "bridges": ordered.len(),
            }));
            Ok(())
        }
        Ok(false) => {
            emit(&json!({ "event": "tier", "tier": "T5", "pass": false }));
            Err(format!(
                "bootstrapped, but {host} did not confirm the path goes through Tor",
                host = opts.check_host
            ))
        }
        Err(err) => {
            emit(&json!({ "event": "tier", "tier": "T5", "pass": false }));
            Err(format!(
                "bootstrapped, but the end-to-end check failed: {err}"
            ))
        }
    }
}

fn still_current(tor: &Arc<Mutex<Child>>) -> bool {
    SESSION
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|s| Arc::ptr_eq(&s.tor, tor)))
        .unwrap_or(false)
}

/// The sidecar as it is shipped: `lib<name>.so` inside an APK, plain `<name>` on a desktop.
fn sidecar_path(pt_dir: &Path, sidecar: &str) -> Option<PathBuf> {
    let bundled = pt_dir.join(format!("lib{sidecar}.so"));
    if bundled.is_file() {
        return Some(bundled);
    }
    let plain = pt_dir.join(sidecar);
    plain.is_file().then_some(plain)
}

fn quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn free_port() -> Option<u16> {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .ok()?
        .local_addr()
        .ok()?
        .port()
        .into()
}

fn read_cookie(data: &Path, tor: &Arc<Mutex<Child>>) -> Option<Vec<u8>> {
    let path = data.join("control_auth_cookie");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() >= 32 {
                return Some(bytes[..32].to_vec());
            }
        }
        if let Ok(mut guard) = tor.lock() {
            if let Ok(Some(_)) = guard.try_wait() {
                break;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    None
}

/// Last lines of tor's own log, for the failure report. Stays on the device unless the user
/// shares it; it can contain bridge addresses.
fn log_tail(work_dir: &Path) -> String {
    let mut lines: Vec<String> = fs::read_to_string(work_dir.join("tor.log"))
        .map(|text| {
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let skip = lines.len().saturating_sub(6);
    lines.drain(..skip);
    lines.join(" | ")
}

fn watch_control(addr: SocketAddr, cookie: Vec<u8>, tx: mpsc::Sender<Value>) {
    let mut stream = None;
    for _ in 0..60 {
        if let Ok(s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
            stream = Some(s);
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    let Some(mut stream) = stream else { return };
    if stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .is_err()
    {
        return;
    }
    let Ok(copy) = TcpStream::try_clone(&stream) else {
        return;
    };
    let mut reader = BufReader::new(copy);
    let hex: String = cookie.iter().map(|b| format!("{b:02X}")).collect();
    let hello = format!("AUTHENTICATE {hex}\r\nSETEVENTS STATUS_CLIENT NOTICE WARN ERR\r\n");
    if stream.write_all(hello.as_bytes()).is_err() {
        return;
    }
    let mut line = String::new();
    let mut idle = 0u32;
    loop {
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                idle = 0;
                if let Some(event) = parse_event(&line) {
                    let _ = tx.send(event);
                }
                line.clear();
            }
            Err(_) => {
                idle += 1;
                if idle > 600 {
                    break;
                }
            }
        }
    }
    let _ = tx.send(json!({ "event": "control_lost" }));
}

/// Control-port `650` lines: bootstrap progress becomes `progress`, warnings become `log`.
fn parse_event(line: &str) -> Option<Value> {
    let line = line.trim_end_matches(['\r', '\n']);
    let mut fields = line.splitn(5, ' ');
    if fields.next()? != "650" {
        return None;
    }
    let severity = fields.next()?;
    let topic = fields.next()?;
    let action = fields.next()?;
    let rest = fields.next().unwrap_or("");
    match (topic, action) {
        ("STATUS_CLIENT", "BOOTSTRAP") => {
            let progress = kv(rest, "PROGRESS")
                .and_then(|v| v.parse::<u8>().ok())
                .unwrap_or(0);
            let summary = kv(rest, "SUMMARY").unwrap_or("");
            if severity == "NOTICE" {
                Some(json!({ "event": "progress", "progress": progress, "summary": summary }))
            } else {
                let warning = kv(rest, "WARNING")
                    .or_else(|| kv(rest, "REASON"))
                    .unwrap_or(summary);
                Some(
                    json!({ "event": "log", "severity": severity, "progress": progress, "detail": warning }),
                )
            }
        }
        ("STATUS_CLIENT", "CIRCUIT_ESTABLISHED") => Some(json!({ "event": "circuit" })),
        ("STATUS_CLIENT", _) => None,
        _ => Some(
            json!({ "event": "log", "severity": severity, "detail": format!("{topic} {action} {rest}") }),
        ),
    }
}

/// `KEY=VALUE` or `KEY="quoted value"` inside a control-port event.
fn kv<'a>(rest: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    let start = if let Some(stripped) = rest.strip_prefix(&prefix) {
        stripped
    } else {
        let marker = format!(" {key}=");
        &rest[rest.find(&marker)? + 1..]
    };
    let value = &start[key.len() + 1..];
    if let Some(quoted) = value.strip_prefix('"') {
        Some(&quoted[..quoted.find('"')?])
    } else {
        Some(value.split(' ').next().unwrap_or(""))
    }
}

fn tls_config() -> Arc<rustls::ClientConfig> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// T2: a full TLS 1.2/1.3 handshake with certificate verification against the name.
fn tls_handshake(addr: SocketAddr, server_name: &str, timeout: Duration) -> Result<(), String> {
    let name: rustls::pki_types::ServerName<'static> = server_name
        .to_owned()
        .try_into()
        .map_err(|_| "not a valid server name".to_string())?;
    let mut conn =
        rustls::ClientConnection::new(tls_config(), name).map_err(|err| err.to_string())?;
    let mut tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|err| err.to_string())?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|err| err.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|err| err.to_string())?;
    conn.complete_io(&mut tcp).map_err(|err| err.to_string())?;
    if conn.is_handshaking() {
        return Err("handshake did not complete".into());
    }
    Ok(())
}

/// T5: through tor's SOCKS port, fetch `https://<host>/api/ip` and read its `IsTor` verdict.
fn tor_check(socks: SocketAddr, host: &str, timeout: Duration) -> Result<bool, String> {
    let mut tcp = TcpStream::connect_timeout(&socks, Duration::from_secs(5))
        .map_err(|err| format!("socks port: {err}"))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|err| err.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|err| err.to_string())?;
    tcp.write_all(&[0x05, 0x01, 0x00])
        .map_err(|err| format!("socks greeting: {err}"))?;
    let mut method = [0u8; 2];
    tcp.read_exact(&mut method)
        .map_err(|err| format!("socks greeting reply: {err}"))?;
    if method != [0x05, 0x00] {
        return Err(format!("socks: no acceptable method ({})", method[1]));
    }
    let name = host.as_bytes();
    if name.is_empty() || name.len() > 255 {
        return Err("check host is not a valid name".into());
    }
    let mut request = vec![0x05, 0x01, 0x00, 0x03, name.len() as u8];
    request.extend_from_slice(name);
    request.extend_from_slice(&443u16.to_be_bytes());
    tcp.write_all(&request)
        .map_err(|err| format!("socks connect: {err}"))?;
    let mut head = [0u8; 4];
    tcp.read_exact(&mut head)
        .map_err(|err| format!("socks connect reply: {err}"))?;
    if head[1] != 0x00 {
        return Err(format!("socks: connect refused, code {}", head[1]));
    }
    let skip = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len)
                .map_err(|err| format!("socks connect reply: {err}"))?;
            usize::from(len[0])
        }
        kind => return Err(format!("socks: unknown address type {kind}")),
    };
    let mut rest = vec![0u8; skip + 2];
    tcp.read_exact(&mut rest)
        .map_err(|err| format!("socks connect reply: {err}"))?;

    let name: rustls::pki_types::ServerName<'static> = host
        .to_owned()
        .try_into()
        .map_err(|_| "not a valid check host".to_string())?;
    let mut conn =
        rustls::ClientConnection::new(tls_config(), name).map_err(|err| format!("tls: {err}"))?;
    let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
    write!(
        tls,
        "GET /api/ip HTTP/1.1\r\nHost: {host}\r\nUser-Agent: HopForge\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .map_err(|err| format!("tls write: {err}"))?;
    tls.flush().map_err(|err| format!("tls flush: {err}"))?;
    let mut response = Vec::new();
    // A timeout after the body arrived is not a failure for us; look at what was read.
    let _ = tls.read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response);
    let first = text.lines().next().unwrap_or("nothing");
    if !(first.starts_with("HTTP/1.1 200") || first.starts_with("HTTP/1.0 200")) {
        return Err(format!("the check host answered {first:?}"));
    }
    Ok(text.contains("\"IsTor\":true") || text.contains("\"IsTor\": true"))
}
