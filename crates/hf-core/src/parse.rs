//! Strict bridge-line parsing.
//!
//! Grammar (per line; the `Bridge` keyword is optional because feeds print both forms):
//!
//! ```text
//! [ "Bridge" ] [ transport ] IP:ORPort [ FINGERPRINT ] [ key=value ]...
//! ```
//!
//! The parser is hand-written and total: every input maps to a [`Bridge`] or a [`ParseError`],
//! never to a panic. It rejects what the shipped client would refuse or mis-dial, and records
//! everything merely suspicious as an [`Issue`] so the ranker can see it.
//!
//! Argument names and required-ness were read in the pinned client sources (`pt-sidecars/go.mod`):
//!
//! | transport  | required                                    | optional                                     |
//! |------------|---------------------------------------------|----------------------------------------------|
//! | obfs4      | `cert`, or legacy `node-id` + `public-key`; `iat-mode` |                                   |
//! | webtunnel  | `url` (http/https)                          | `ver` (not read by the client), `cert`, `servername` |
//! | snowflake  | `url`                                       | `front`/`fronts`, `ice`, `ampcache`, `utls-imitate`, ... |
//! | meek_lite  | `url`                                       | `front`, `utls`, legacy `targets`            |
//! | conjure    | —                                           | `url`, `front`/`fronts`, `registrar`, `transport`, ... |
//! | dnstt      | `domain`, `pubkey`, one of `doh`/`dot`/`udp` | `utls`                                     |
//!
//! Two facts from those sources shape the model:
//!
//! * **A client returns the first value for a repeated key** (goptlib `Args.Get`), so the parser
//!   keeps the first value too.
//! * **The webtunnel client never connects to the address on the line.** It resolves the host of
//!   `url` and dials that (`webtunnel/main/client/main.go`, `getAddressesFromHostname`). Every
//!   feed therefore writes a placeholder there (`127.0.0.1:15000`, `[2001:db8::…]`), and the
//!   bridge's identity ignores the line address for webtunnel and the fronted transports.
//!
//! `dnstt` lines are not a Tor construct: `dnstt-client` is not a managed pluggable transport. It
//! forwards a local TCP port to the bridge ORPort over DNS, and Tor is pointed at that port with a
//! vanilla line (dnstt README, "Tor bridge"). The `key=value` encoding is the convention the
//! bridge collectors use.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::transport::{DialTarget, Transport};

/// Longest line accepted from a feed, in bytes. The longest legitimate line seen in the feeds is
/// 498 bytes (a Snowflake line with a full STUN list).
pub const MAX_LINE_LEN: usize = 2048;
/// Most `key=value` pairs accepted on one line.
pub const MAX_ARGS: usize = 24;
/// Longest accepted value of one argument, in bytes.
pub const MAX_ARG_VALUE_LEN: usize = 512;
/// Length of a Tor relay identity fingerprint in bytes.
pub const FINGERPRINT_LEN: usize = 20;
/// Length of an obfs4 `cert`, in bytes: the 20-byte node id followed by the 32-byte public key
/// (lyrebird `transports/obfs4/statefile.go`, `certLength`).
pub const OBFS4_CERT_LEN: usize = 52;
/// Length of an obfs4 `cert` as written in a bridge line: base64 without the `==` padding
/// lyrebird appends before decoding.
pub const OBFS4_CERT_CHARS: usize = 70;

// ---------------------------------------------------------------------------------------------
// Errors and issues
// ---------------------------------------------------------------------------------------------

/// Why a line was rejected. Stable identifiers: they are stored, exported and translated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reject {
    /// Empty line or comment.
    Empty,
    /// Longer than [`MAX_LINE_LEN`].
    TooLong,
    /// The first token is neither a transport name nor an address.
    BadTransportName,
    /// The line has no address after the transport.
    MissingEndpoint,
    /// The address is not an IP literal in `IP:port` / `[IPv6]:port` form.
    BadAddress,
    /// Port is missing, zero or above 65535.
    BadPort,
    /// A hostname was given where Tor requires an IP literal.
    HostnameNotAllowed,
    /// Loopback, private, carrier-grade NAT, link-local or reserved space on a transport that
    /// dials the address on the line.
    NonRoutable,
    /// A documentation/example address (RFC 5737 / RFC 3849) on a transport that dials the
    /// address on the line.
    Placeholder,
    /// Fingerprint is not 40 hex characters.
    BadFingerprint,
    /// Malformed `key=value` token.
    BadArg,
    /// The obfs4 `cert` (or legacy `node-id` / `public-key`) is not what lyrebird decodes.
    BadCert,
    /// An argument required by the transport is missing or has an invalid value.
    MissingRequiredArg,
    /// More than [`MAX_ARGS`] arguments.
    TooManyArgs,
    /// A bare token where neither a fingerprint nor `key=value` may appear.
    UnexpectedToken,
}

/// Non-fatal findings about a parsed bridge. The bridge stays usable; the ranker and the UI see
/// these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
    /// No fingerprint on the line: the relay's identity is not pinned.
    MissingFingerprint,
    /// obfs4 line without `iat-mode`; repaired to `iat-mode=0` (lyrebird requires the argument).
    MissingIatMode,
    /// An argument this parser does not know for the transport (kept, passed through).
    UnknownArg,
    /// The transport is well-formed but HopForge ships no client for it.
    UnsupportedTransport,
    /// The same argument key appears twice; the first value was kept, as the client would.
    DuplicateArg,
    /// A fronted transport without any `front`/`fronts` argument: the broker is reached directly.
    NoFront,
    /// A `fingerprint=` argument that disagrees with the fingerprint on the line.
    FingerprintMismatch,
}

/// A rejected line with its position, for the UI ("N lines skipped and why").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejected {
    /// 1-based line number in the input.
    pub line_no: usize,
    /// The offending line, trimmed and truncated to 200 bytes.
    pub line: String,
    /// Why it was rejected.
    pub reason: Reject,
}

/// Error returned by [`parse_line`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{reason:?}: {detail}")]
pub struct ParseError {
    /// Machine-readable reason.
    pub reason: Reject,
    /// Human-readable detail, safe to show (names the offending token, nothing else).
    pub detail: String,
}

impl ParseError {
    fn new(reason: Reject, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------------------------

/// A 40-hex-character relay identity fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; FINGERPRINT_LEN]);

impl Fingerprint {
    /// The raw 20 bytes.
    pub const fn as_bytes(&self) -> &[u8; FINGERPRINT_LEN] {
        &self.0
    }

    /// Uppercase hex, the way Tor prints fingerprints.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02X}")).collect()
    }
}

impl FromStr for Fingerprint {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !is_hex(s, FINGERPRINT_LEN * 2) {
            return Err(ParseError::new(
                Reject::BadFingerprint,
                "fingerprint must be 40 hex characters",
            ));
        }
        let mut out = [0u8; FINGERPRINT_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let hi = (s.as_bytes()[i * 2] as char).to_digit(16).unwrap_or(0);
            let lo = (s.as_bytes()[i * 2 + 1] as char).to_digit(16).unwrap_or(0);
            *slot = ((hi << 4) | lo) as u8;
        }
        Ok(Self(out))
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// How the endpoint address classifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddrClass {
    /// Globally routable unicast.
    Public,
    /// RFC 5737 / RFC 3849 documentation space (`192.0.2.0/24`, `198.51.100.0/24`,
    /// `203.0.113.0/24`, `2001:db8::/32`). Fronted transports put one of these in the line.
    Documentation,
    /// Loopback.
    Loopback,
    /// RFC 1918 / RFC 4193 / carrier-grade NAT / link-local.
    Private,
    /// Unspecified (`0.0.0.0`, `::`), multicast, reserved or broadcast.
    Reserved,
}

/// Classifies an address for the purpose of "is this a real bridge address?".
pub fn classify_addr(ip: IpAddr) -> AddrClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return classify_v4(v4);
            }
            let seg = v6.segments();
            if v6.is_loopback() {
                AddrClass::Loopback
            } else if v6.is_unspecified() || v6.is_multicast() {
                AddrClass::Reserved
            } else if seg[0] & 0xfe00 == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80 {
                AddrClass::Private
            } else if seg[0] == 0x2001 && seg[1] == 0x0db8 {
                AddrClass::Documentation
            } else {
                AddrClass::Public
            }
        }
    }
}

fn classify_v4(v4: Ipv4Addr) -> AddrClass {
    let o = v4.octets();
    let is_doc = (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113);
    let is_private = o[0] == 10
        || (o[0] == 172 && o[1] & 0xf0 == 16)
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 100 && o[1] & 0xc0 == 64)
        || (o[0] == 169 && o[1] == 254);
    if is_doc {
        AddrClass::Documentation
    } else if o[0] == 127 {
        AddrClass::Loopback
    } else if is_private {
        AddrClass::Private
    } else if o[0] == 0 || o[0] >= 224 {
        AddrClass::Reserved
    } else {
        AddrClass::Public
    }
}

/// Where a reachability probe has to connect for this bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeTarget {
    /// The IP:port written on the line (vanilla, obfs4).
    Addr(SocketAddr),
    /// A host name to resolve, its TCP port, and the TLS server name to send. Webtunnel dials
    /// the host of its `url`, not the address on the line.
    Host {
        /// Host name from the `url`.
        host: String,
        /// TCP port (the URL's, else 443 for https and 80 for http).
        port: u16,
        /// TLS server name: the `servername` argument if present, else the URL host.
        server_name: String,
    },
}

/// One parsed bridge line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bridge {
    /// Transport named on the line.
    pub transport: Transport,
    /// Endpoint exactly as written (a placeholder for fronted transports and webtunnel).
    pub addr: SocketAddr,
    /// Relay identity fingerprint, when the line pins one.
    pub fingerprint: Option<Fingerprint>,
    /// `key=value` arguments, sorted by key, lower-cased keys, first value kept on repeats.
    /// Contains the repaired `iat-mode=0` for obfs4 lines that omitted it.
    pub args: BTreeMap<String, String>,
    /// The original line, trimmed.
    pub raw: String,
    /// Non-fatal findings.
    pub issues: Vec<Issue>,
}

impl Bridge {
    /// The line in Tor's form: `Bridge <transport> <addr> <FP> <k=v ...>`.
    pub fn tor_line(&self) -> String {
        let mut out = String::with_capacity(self.raw.len() + 32);
        out.push_str("Bridge ");
        if !matches!(self.transport, Transport::Vanilla) {
            out.push_str(&self.transport.name());
            out.push(' ');
        }
        out.push_str(&self.addr.to_string());
        if let Some(fp) = &self.fingerprint {
            out.push(' ');
            out.push_str(&fp.to_hex());
        }
        for (k, v) in &self.args {
            out.push(' ');
            out.push_str(k);
            out.push('=');
            out.push_str(v);
        }
        out
    }

    /// The fields that define which bridge this is and how it is reached, independent of
    /// spacing, argument order, the optional `Bridge` keyword, key case and the placeholder
    /// address on fronted and webtunnel lines.
    pub fn endpoint_key(&self) -> String {
        let mut out = String::with_capacity(128);
        out.push_str(&self.transport.name());
        out.push('|');
        if self.transport.line_addr_is_identity() {
            out.push_str(&self.addr.to_string());
        }
        for (key, value) in identity_args(self) {
            out.push('|');
            out.push_str(&key);
            out.push('=');
            out.push_str(&value);
        }
        out
    }

    /// Stable identity: lowercase hex SHA-256 of the endpoint key plus the fingerprint.
    pub fn id(&self) -> String {
        let mut canonical = self.endpoint_key();
        canonical.push('|');
        if let Some(fp) = &self.fingerprint {
            canonical.push_str(&fp.to_hex());
        }
        Sha256::digest(canonical.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Short form of [`Bridge::id`] for lists and logs.
    pub fn short_id(&self) -> String {
        self.id()[..12].to_string()
    }

    /// Value of one argument.
    pub fn arg(&self, key: &str) -> Option<&str> {
        self.args.get(key).map(String::as_str)
    }

    /// The host names a fronted transport is reached through (front domains), for display.
    /// Empty for direct transports.
    pub fn front_hosts(&self) -> Vec<String> {
        for key in ["fronts", "front"] {
            if let Some(v) = self.arg(key) {
                return v
                    .split(',')
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
        Vec::new()
    }

    /// The `url` argument split into the pieces a probe needs.
    pub fn url(&self) -> Option<Url<'_>> {
        self.arg("url").and_then(Url::parse)
    }

    /// Where a reachability probe connects for this bridge, when HopForge can connect anywhere
    /// without the transport's client.
    pub fn probe_target(&self) -> Option<ProbeTarget> {
        match self.transport.dial_target() {
            DialTarget::LineAddress => Some(ProbeTarget::Addr(self.addr)),
            DialTarget::UrlHost => {
                let url = self.url()?;
                Some(ProbeTarget::Host {
                    host: url.host.to_string(),
                    port: url.port,
                    server_name: self.arg("servername").unwrap_or(url.host).to_string(),
                })
            }
            DialTarget::Fronted | DialTarget::None => None,
        }
    }
}

/// The pieces of an `url=` argument HopForge needs. Not a general URL parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Url<'a> {
    /// Whether the scheme is `https`.
    pub secure: bool,
    /// Host name as written (a name, or an IP literal without brackets).
    pub host: &'a str,
    /// Explicit port, or the scheme's default (443 / 80).
    pub port: u16,
    /// Path with the leading `/`, empty if absent.
    pub path: &'a str,
}

impl<'a> Url<'a> {
    /// Parses `http://host[:port][/path]` or `https://host[:port][/path]`.
    pub fn parse(value: &'a str) -> Option<Self> {
        let (secure, rest) = if let Some(rest) = value.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = value.strip_prefix("http://") {
            (false, rest)
        } else {
            return None;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let (host, port_str) = split_host_port(authority)?;
        let port = match port_str {
            Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0)?,
            None if secure => 443,
            None => 80,
        };
        let path = rest
            .strip_prefix(authority)
            .unwrap_or("")
            .split(['?', '#'])
            .next()
            .unwrap_or("");
        Some(Self {
            secure,
            host,
            port,
            path,
        })
    }
}

/// Splits `host` or `host:port` or `[v6]` or `[v6]:port` into its parts.
fn split_host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    if authority.is_empty() || authority.len() > 253 || authority.contains('@') {
        return None;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        host.parse::<Ipv6Addr>().ok()?;
        let port = match after {
            "" => None,
            other => Some(other.strip_prefix(':')?),
        };
        return Some((host, port));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
        || host.starts_with('.')
        || host.ends_with('.')
    {
        return None;
    }
    Some((host, port))
}

/// Result of parsing a whole feed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedSet {
    /// Unique bridges in input order (first occurrence wins).
    pub bridges: Vec<Bridge>,
    /// Lines that were not accepted, in input order.
    pub rejected: Vec<Rejected>,
    /// Lines that named a bridge already in the set. Their fingerprint or missing arguments were
    /// merged into the bridge kept.
    pub duplicates: usize,
}

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

/// Characters that are invisible or directional and have no business inside a bridge line.
const HIDDEN_CHARS: [char; 5] = ['\u{feff}', '\u{200b}', '\u{200c}', '\u{200e}', '\u{200f}'];

/// Parses one bridge line.
pub fn parse_line(line: &str) -> Result<Bridge, ParseError> {
    let raw = line
        .trim_matches(|c: char| c.is_whitespace() || HIDDEN_CHARS.contains(&c))
        .to_string();
    if raw.is_empty() || raw.starts_with('#') {
        return Err(ParseError::new(Reject::Empty, "empty line or comment"));
    }
    if raw.len() > MAX_LINE_LEN {
        return Err(ParseError::new(
            Reject::TooLong,
            "line exceeds the length limit",
        ));
    }
    if raw
        .chars()
        .any(|c| c.is_control() || HIDDEN_CHARS.contains(&c))
    {
        return Err(ParseError::new(
            Reject::BadArg,
            "control or invisible character inside the line",
        ));
    }

    let mut tokens = raw.split_whitespace();
    let mut token = tokens.next().expect("a non-empty line yields a token");
    if token.eq_ignore_ascii_case("Bridge") {
        token = tokens.next().ok_or_else(|| {
            ParseError::new(
                Reject::MissingEndpoint,
                "line ends after the Bridge keyword",
            )
        })?;
    }

    // A transport token, if present. An endpoint always contains ':' or starts with '['.
    let transport = if token.starts_with('[') || token.contains(':') {
        Transport::Vanilla
    } else if parse_ipv4(token).is_some() {
        return Err(ParseError::new(
            Reject::BadAddress,
            "address must be IP:port",
        ));
    } else {
        let t = Transport::parse(token).ok_or_else(|| {
            ParseError::new(
                Reject::BadTransportName,
                "not a transport name or an address",
            )
        })?;
        token = tokens.next().ok_or_else(|| {
            ParseError::new(Reject::MissingEndpoint, "no address after the transport")
        })?;
        t
    };

    let addr = parse_endpoint(token)?;

    let mut fingerprint: Option<Fingerprint> = None;
    let mut args: BTreeMap<String, String> = BTreeMap::new();
    let mut issues = Vec::new();
    let mut fingerprint_seen = false;
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            if args.len() >= MAX_ARGS {
                return Err(ParseError::new(Reject::TooManyArgs, "too many arguments"));
            }
            if !is_arg_key(key) || value.is_empty() || value.len() > MAX_ARG_VALUE_LEN {
                return Err(ParseError::new(
                    Reject::BadArg,
                    "malformed key=value argument",
                ));
            }
            let key = key.to_ascii_lowercase();
            if args.contains_key(&key) {
                issues.push(Issue::DuplicateArg);
            } else {
                args.insert(key, value.to_string());
            }
        } else if !fingerprint_seen && looks_like_fingerprint(token) {
            fingerprint = Some(token.parse()?);
            fingerprint_seen = true;
        } else {
            return Err(ParseError::new(
                Reject::UnexpectedToken,
                "expected a fingerprint or key=value argument",
            ));
        }
    }

    let mut bridge = Bridge {
        transport,
        addr,
        fingerprint,
        args,
        raw,
        issues,
    };
    validate(&mut bridge)?;
    bridge.issues.sort_unstable();
    bridge.issues.dedup();
    Ok(bridge)
}

/// Parses many lines, merging lines that name the same bridge and collecting rejections.
pub fn parse_lines<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> ParsedSet {
    let mut set = ParsedSet::default();
    // Position in `set.bridges` of the bridge kept for each endpoint key, and of each bridge
    // kept for each full identity.
    let mut by_endpoint: HashMap<String, usize> = HashMap::new();
    let mut by_id: HashSet<String> = HashSet::new();
    for (idx, line) in lines.into_iter().enumerate() {
        let parsed = match parse_line(line.as_ref()) {
            Ok(bridge) => bridge,
            Err(err) if err.reason == Reject::Empty => continue,
            Err(err) => {
                set.rejected.push(Rejected {
                    line_no: idx + 1,
                    line: truncate(line.as_ref().trim(), 200),
                    reason: err.reason,
                });
                continue;
            }
        };
        let endpoint = parsed.endpoint_key();
        let id = parsed.id();
        match by_endpoint.get(&endpoint) {
            // Same bridge again, unless the two lines pin different relays: then they are
            // different bridges that merely share an endpoint.
            Some(&position) if fingerprint_agrees(&set.bridges[position], &parsed.fingerprint) => {
                let kept = &mut set.bridges[position];
                merge_into(kept, &parsed);
                by_id.insert(kept.id());
                set.duplicates += 1;
            }
            _ if by_id.contains(&id) => set.duplicates += 1,
            _ => {
                by_endpoint.insert(endpoint, set.bridges.len());
                by_id.insert(id);
                set.bridges.push(parsed);
            }
        }
    }
    set
}

/// Whether a line's fingerprint names the relay already kept for its endpoint.
fn fingerprint_agrees(kept: &Bridge, fingerprint: &Option<Fingerprint>) -> bool {
    match (kept.fingerprint, *fingerprint) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Fills in what an earlier line for the same bridge lacks from a later one: the fingerprint,
/// and client-side options that do not change which bridge it is.
fn merge_into(kept: &mut Bridge, later: &Bridge) {
    if kept.fingerprint.is_none() {
        kept.fingerprint = later.fingerprint;
        kept.issues
            .retain(|issue| *issue != Issue::MissingFingerprint);
    }
    let ignored = ignored_in_identity(&kept.transport);
    for (key, value) in &later.args {
        if ignored.contains(&key.as_str()) {
            kept.args
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    kept.issues.extend(later.issues.iter().copied());
    if kept.fingerprint.is_some() {
        kept.issues
            .retain(|issue| *issue != Issue::MissingFingerprint);
    }
    if kept.args.contains_key("iat-mode") {
        kept.issues.retain(|issue| *issue != Issue::MissingIatMode);
    }
    kept.issues.sort_unstable();
    kept.issues.dedup();
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn is_arg_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key.starts_with(|c: char| c == '_' || c.is_ascii_alphabetic())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn looks_like_fingerprint(token: &str) -> bool {
    is_hex(token, FINGERPRINT_LEN * 2)
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn parse_endpoint(token: &str) -> Result<SocketAddr, ParseError> {
    let (host, port, bracketed) = match token.strip_prefix('[') {
        Some(rest) => {
            let (host, port) = rest.split_once("]:").ok_or_else(|| {
                ParseError::new(Reject::BadAddress, "unterminated [IPv6] address")
            })?;
            (host, port, true)
        }
        None if token.bytes().filter(|b| *b == b':').count() > 1 => {
            return Err(ParseError::new(
                Reject::BadAddress,
                "IPv6 must be written in brackets",
            ));
        }
        None => {
            let (host, port) = token
                .split_once(':')
                .ok_or_else(|| ParseError::new(Reject::BadAddress, "address must be IP:port"))?;
            (host, port, false)
        }
    };
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::new(Reject::BadPort, "port must be 1..=65535"));
    }
    let port: u16 = port
        .parse()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| ParseError::new(Reject::BadPort, "port must be 1..=65535"))?;
    let ip: IpAddr = if bracketed {
        host.parse::<Ipv6Addr>()
            .map(IpAddr::V6)
            .map_err(|_| ParseError::new(Reject::BadAddress, "malformed IPv6 address"))?
    } else {
        parse_ipv4(host).map(IpAddr::V4).ok_or_else(|| {
            ParseError::new(
                Reject::HostnameNotAllowed,
                "endpoint must be an IP literal, not a host name",
            )
        })?
    };
    Ok(SocketAddr::new(ip, port))
}

/// Strict dotted-quad: four decimal octets 0–255, no signs, no leading zeros.
fn parse_ipv4(host: &str) -> Option<Ipv4Addr> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in host.split('.') {
        let slot = octets.get_mut(count)?;
        if part.is_empty() || part.len() > 3 || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        *slot = part.parse().ok()?;
        count += 1;
    }
    (count == 4).then_some(Ipv4Addr::from(octets))
}

// ---------------------------------------------------------------------------------------------
// Per-transport validation
// ---------------------------------------------------------------------------------------------

/// Whether `cert` is what lyrebird decodes: 70 characters of the standard base64 alphabet,
/// without padding, standing for exactly 52 bytes (`base64.StdEncoding.DecodeString(cert + "==")`).
fn is_obfs4_cert(cert: &str) -> bool {
    cert.len() == OBFS4_CERT_CHARS
        && cert
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        && base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(cert)
            .is_ok_and(|bytes| bytes.len() == OBFS4_CERT_LEN)
}

fn is_domain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value != "."
        && value.split('.').all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// Known argument keys per transport. Anything else is kept but flagged [`Issue::UnknownArg`].
fn known_args(transport: &Transport) -> &'static [&'static str] {
    match transport {
        Transport::Obfs4 => &["cert", "iat-mode", "node-id", "public-key"],
        Transport::Webtunnel => &["url", "ver", "cert", "servername"],
        Transport::Snowflake => &[
            "url",
            "front",
            "fronts",
            "ice",
            "ampcache",
            "sqsqueue",
            "sqscreds",
            "max",
            "utls-imitate",
            "utls-nosni",
            "fingerprint",
            "covertdtls-config",
            "covertdtls-fingerprint",
        ],
        Transport::MeekLite => &["url", "front", "utls", "targets"],
        Transport::Conjure => &[
            "url",
            "front",
            "fronts",
            "transport",
            "registrar",
            "ampcache",
            "utls-nosni",
            "utls-imitate",
            "stun",
        ],
        Transport::Dnstt => &[
            "doh",
            "dot",
            "udp",
            "pubkey",
            "pubkey-file",
            "domain",
            "utls",
        ],
        Transport::Vanilla | Transport::Unknown(_) => &[],
    }
}

/// Arguments that only change how one client talks to the same bridge and are left out of the
/// endpoint key.
fn ignored_in_identity(transport: &Transport) -> &'static [&'static str] {
    match transport {
        Transport::Obfs4 => &["iat-mode"],
        Transport::Webtunnel => &["ver"],
        Transport::Snowflake => &[
            "max",
            "utls-imitate",
            "utls-nosni",
            "covertdtls-config",
            "covertdtls-fingerprint",
        ],
        Transport::MeekLite => &["utls"],
        Transport::Conjure => &["utls-nosni", "utls-imitate"],
        Transport::Dnstt => &["utls"],
        Transport::Vanilla | Transport::Unknown(_) => &[],
    }
}

/// Arguments whose value is an unordered list; normalized so `front=a,b` and `fronts=b,a` are
/// the same endpoint.
fn list_args() -> &'static [&'static str] {
    &["front", "fronts", "ice"]
}

/// The `(key, value)` pairs that take part in the endpoint key, in a canonical order.
fn identity_args(bridge: &Bridge) -> Vec<(String, String)> {
    let ignored = ignored_in_identity(&bridge.transport);
    let lists = list_args();
    let mut out: Vec<(String, String)> = Vec::new();
    for (key, value) in &bridge.args {
        if ignored.contains(&key.as_str()) {
            continue;
        }
        let key = if key == "front" {
            "fronts"
        } else {
            key.as_str()
        };
        if lists.contains(&key) {
            let mut items: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|i| !i.is_empty())
                .collect();
            items.sort_unstable();
            items.dedup();
            out.push((key.to_string(), items.join(",")));
        } else {
            out.push((key.to_string(), value.clone()));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn missing(what: &'static str) -> ParseError {
    ParseError::new(Reject::MissingRequiredArg, what)
}

fn bad_arg(what: &'static str) -> ParseError {
    ParseError::new(Reject::BadArg, what)
}

fn validate(bridge: &mut Bridge) -> Result<(), ParseError> {
    let transport = bridge.transport.clone();

    if transport.dial_target() == DialTarget::LineAddress {
        match classify_addr(bridge.addr.ip()) {
            AddrClass::Public => {}
            AddrClass::Documentation => {
                return Err(ParseError::new(
                    Reject::Placeholder,
                    "documentation address (RFC 5737/3849) on a transport that dials the address on the line",
                ));
            }
            _ => {
                return Err(ParseError::new(
                    Reject::NonRoutable,
                    "loopback/private/reserved address on a transport that dials the address on the line",
                ));
            }
        }
    }

    if bridge.fingerprint.is_none() {
        bridge.issues.push(Issue::MissingFingerprint);
    }
    if !transport.is_dialable() && !matches!(transport, Transport::Vanilla) {
        bridge.issues.push(Issue::UnsupportedTransport);
    }

    let known = known_args(&transport);
    if bridge.args.keys().any(|key| !known.contains(&key.as_str())) {
        bridge.issues.push(Issue::UnknownArg);
    }

    match &transport {
        Transport::Obfs4 => validate_obfs4(bridge)?,
        Transport::Webtunnel | Transport::Snowflake | Transport::MeekLite => {
            match bridge.url() {
                Some(_) => {}
                None if bridge.args.contains_key("url") => {
                    return Err(bad_arg("url must be an http(s) URL with a host"))
                }
                None => return Err(missing("bridge line has no url")),
            }
            if transport.is_fronted() && bridge.front_hosts().is_empty() {
                bridge.issues.push(Issue::NoFront);
            }
            if matches!(transport, Transport::Snowflake) {
                validate_snowflake(bridge)?;
            }
        }
        Transport::Conjure => {
            if bridge.args.contains_key("url") && bridge.url().is_none() {
                return Err(bad_arg("url must be an http(s) URL with a host"));
            }
            if bridge.front_hosts().is_empty() {
                bridge.issues.push(Issue::NoFront);
            }
        }
        Transport::Dnstt => validate_dnstt(bridge)?,
        Transport::Vanilla | Transport::Unknown(_) => {}
    }
    Ok(())
}

fn validate_obfs4(bridge: &mut Bridge) -> Result<(), ParseError> {
    match bridge.arg("cert") {
        Some(cert) if is_obfs4_cert(cert) => {}
        Some(_) => {
            return Err(ParseError::new(
                Reject::BadCert,
                "cert is not 70 base64 characters for 52 bytes",
            ))
        }
        None => {
            match bridge.arg("node-id") {
                Some(node_id) if is_hex(node_id, 40) => {}
                Some(_) => {
                    return Err(ParseError::new(
                        Reject::BadCert,
                        "node-id must be 40 hex characters",
                    ))
                }
                None => return Err(missing("obfs4 line has no cert")),
            }
            match bridge.arg("public-key") {
                Some(key) if is_hex(key, 64) => {}
                Some(_) => {
                    return Err(ParseError::new(
                        Reject::BadCert,
                        "public-key must be 64 hex characters",
                    ))
                }
                None => return Err(missing("obfs4 line has no cert")),
            }
        }
    }
    match bridge.arg("iat-mode") {
        None => {
            bridge.args.insert("iat-mode".to_string(), "0".to_string());
            bridge.issues.push(Issue::MissingIatMode);
        }
        Some(mode) if !matches!(mode, "0" | "1" | "2") => {
            return Err(bad_arg("iat-mode must be 0, 1 or 2"))
        }
        Some(_) => {}
    }
    Ok(())
}

fn validate_snowflake(bridge: &mut Bridge) -> Result<(), ParseError> {
    match bridge.arg("fingerprint") {
        Some(fp) if !is_hex(fp, 40) => {
            return Err(bad_arg("fingerprint must be 40 hex characters"))
        }
        Some(fp)
            if bridge
                .fingerprint
                .is_some_and(|token| !token.to_hex().eq_ignore_ascii_case(fp)) =>
        {
            bridge.issues.push(Issue::FingerprintMismatch);
        }
        _ => {}
    }
    if let Some(max) = bridge.arg("max") {
        if max.parse::<u32>().is_err() || max.parse::<u32>() == Ok(0) {
            return Err(bad_arg("max must be a positive integer"));
        }
    }
    Ok(())
}

fn validate_dnstt(bridge: &mut Bridge) -> Result<(), ParseError> {
    if !bridge.arg("domain").is_some_and(is_domain) {
        return Err(missing("dnstt line has no valid domain"));
    }
    match bridge.arg("pubkey") {
        Some(key) if is_hex(key, 64) => {}
        Some(_) => return Err(bad_arg("pubkey must be 64 hex characters")),
        None if bridge.args.contains_key("pubkey-file") => {}
        None => return Err(missing("dnstt line has no pubkey")),
    }
    let resolvers = ["doh", "dot", "udp"];
    if resolvers.iter().all(|k| bridge.arg(k).is_none()) {
        return Err(missing("dnstt line needs one of doh, dot or udp"));
    }
    if let Some(doh) = bridge.arg("doh") {
        if Url::parse(doh).is_none() {
            return Err(bad_arg("doh must be an http(s) URL"));
        }
    }
    for key in ["dot", "udp"] {
        if let Some(addr) = bridge.arg(key) {
            if !split_host_port(addr).is_some_and(|(_, port)| port.is_some()) {
                return Err(bad_arg("dot and udp must be host:port"));
            }
        }
    }
    Ok(())
}
