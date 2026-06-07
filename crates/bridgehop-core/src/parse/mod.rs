//! Bridge-line parsing.
//!
//! A faithful port of OnionHop's bridge-line handling (`BridgeScanService.TryParseEndpoint`,
//! `ExtractFrontHost`, `IsPlaceholderHost`, plus the fingerprint/parameter handling in
//! `TorBridgeManager`), improved to parse a line **once** into a typed [`Bridge`] with a
//! parameter map and a stable id for dedupe — instead of re-scanning the raw string at each step.

use std::collections::BTreeMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::model::{Bridge, Endpoint, Transport};

mod placeholder;
pub use placeholder::is_placeholder_host;

/// Recognized transport tokens (matches OnionHop's `KnownTransports`). A first token outside
/// this set is treated as a `vanilla` line whose first token is the endpoint itself.
const KNOWN_TRANSPORTS: &[&str] = &[
    "obfs4",
    "webtunnel",
    "snowflake",
    "meek_lite",
    "meek-azure",
    "meek",
    "conjure",
    "scramblesuit",
    "obfs3",
    "obfs2",
    "vanilla",
    "dnstt",
];

static IPV4_ENDPOINT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d{1,3}(?:\.\d{1,3}){3}):(\d{1,5})").unwrap());

static IPV6_ENDPOINT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[([0-9a-fA-F:]+)\]:(\d{1,5})").unwrap());

/// `key=value` parameter tokens (cert, iat-mode, url, front, fronts, doh, dot, ver, …).
static KEY_VALUE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)([A-Za-z0-9_-]+)=([^\s]+)").unwrap());

/// Parse a single bridge line into a typed [`Bridge`].
///
/// Returns `None` for blank lines and `#` comments. A leading `Bridge ` (torrc) prefix is
/// stripped. Unknown leading tokens are treated as `vanilla`, mirroring OnionHop.
pub fn parse_bridge_line(line: &str) -> Option<Bridge> {
    let normalized = normalize_line(line)?;

    let first_token = normalized.split_whitespace().next().unwrap_or("");
    let transport = if is_known_token(first_token) {
        first_token
            .parse()
            .expect("Transport::from_str is infallible")
    } else {
        Transport::Vanilla
    };

    let params = extract_params(&normalized);
    let endpoint = extract_endpoint(&normalized);
    let fingerprint = extract_fingerprint(&normalized);
    let front_host = extract_front_host(&params);
    let id = stable_id(&normalized);

    Some(Bridge {
        raw: normalized,
        transport,
        endpoint,
        fingerprint,
        front_host,
        params,
        id,
    })
}

/// Parse many lines, skipping blanks/comments and anything unparseable.
pub fn parse_bridge_lines<'a, I>(lines: I) -> Vec<Bridge>
where
    I: IntoIterator<Item = &'a str>,
{
    lines.into_iter().filter_map(parse_bridge_line).collect()
}

/// Trim, drop blank/comment lines, and strip a leading `Bridge ` prefix.
fn normalize_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let stripped = strip_bridge_prefix(trimmed).unwrap_or(trimmed).trim();
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

/// Strip a case-insensitive leading `Bridge ` token (torrc form).
fn strip_bridge_prefix(s: &str) -> Option<&str> {
    let head = s.get(..7)?;
    head.eq_ignore_ascii_case("Bridge ").then(|| &s[7..])
}

fn is_known_token(token: &str) -> bool {
    KNOWN_TRANSPORTS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(token))
}

/// Extract the first IP:port endpoint (IPv4 preferred, then bracketed IPv6).
fn extract_endpoint(line: &str) -> Option<Endpoint> {
    if let Some(c) = IPV4_ENDPOINT.captures(line) {
        if let Some(port) = valid_port(&c[2]) {
            return Some(Endpoint {
                host: c[1].to_string(),
                port,
                is_ipv6: false,
            });
        }
    }
    if let Some(c) = IPV6_ENDPOINT.captures(line) {
        if let Some(port) = valid_port(&c[2]) {
            return Some(Endpoint {
                host: c[1].to_string(),
                port,
                is_ipv6: true,
            });
        }
    }
    None
}

fn valid_port(s: &str) -> Option<u16> {
    match s.parse::<u32>() {
        Ok(p) if (1..=65535).contains(&p) => Some(p as u16),
        _ => None,
    }
}

/// Collect all `key=value` parameters, keys lowercased for consistent lookup.
fn extract_params(line: &str) -> BTreeMap<String, String> {
    KEY_VALUE
        .captures_iter(line)
        .map(|c| (c[1].to_ascii_lowercase(), c[2].to_string()))
        .collect()
}

/// The relay fingerprint: a standalone 40-hex-digit token, normalized to uppercase.
fn extract_fingerprint(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|tok| tok.len() == 40 && tok.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|s| s.to_ascii_uppercase())
}

/// Pull the broker/front host from a parsed parameter map. Precedence mirrors OnionHop's
/// `ExtractFrontHost`: `url=` → `doh=` → `dot=` (port stripped) → first `fronts=` → `front=`.
fn extract_front_host(params: &BTreeMap<String, String>) -> Option<String> {
    if let Some(url) = params.get("url") {
        if let Some(host) = host_from_url(url) {
            return Some(host);
        }
    }
    if let Some(doh) = params.get("doh") {
        if let Some(host) = host_from_url(doh) {
            return Some(host);
        }
    }
    if let Some(dot) = params.get("dot") {
        // dot is host:port; strip the port for a TLS reachability probe.
        let host = match dot.rfind(':') {
            Some(i) if i > 0 => &dot[..i],
            _ => dot.as_str(),
        };
        if !host.is_empty() {
            return Some(host.to_string());
        }
    }
    if let Some(fronts) = params.get("fronts") {
        if let Some(first) = fronts.split(',').map(str::trim).find(|s| !s.is_empty()) {
            return Some(first.to_string());
        }
    }
    if let Some(front) = params.get("front") {
        let front = front.trim();
        if !front.is_empty() {
            return Some(front.to_string());
        }
    }
    None
}

/// Best-effort host extraction from a URL or `host:port` authority, without pulling in a URL crate.
fn host_from_url(url: &str) -> Option<String> {
    let s = url.trim();
    let after_scheme = s.split_once("://").map(|(_, rest)| rest).unwrap_or(s);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let authority = authority
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(authority);

    let host = if let Some(rest) = authority.strip_prefix('[') {
        // [ipv6]:port -> ipv6
        rest.split(']').next().unwrap_or(rest)
    } else {
        // host or host:port -> host
        authority.split(':').next().unwrap_or(authority)
    };

    let host = host.trim();
    (!host.is_empty()).then(|| host.to_string())
}

/// Deterministic, dependency-free id over the whitespace-collapsed, lowercased line (FNV-1a 64).
fn stable_id(normalized: &str) -> String {
    let canon = normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canon.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_blank_and_comment_lines() {
        assert!(parse_bridge_line("").is_none());
        assert!(parse_bridge_line("   ").is_none());
        assert!(parse_bridge_line("# a comment").is_none());
    }

    #[test]
    fn parses_vanilla_line() {
        let b = parse_bridge_line("1.2.3.4:8080 A7E7616C91B2FD83005B986A816EE9365F1360F4").unwrap();
        assert_eq!(b.transport, Transport::Vanilla);
        let ep = b.endpoint.unwrap();
        assert_eq!(ep.host, "1.2.3.4");
        assert_eq!(ep.port, 8080);
        assert!(!ep.is_ipv6);
        assert_eq!(
            b.fingerprint.as_deref(),
            Some("A7E7616C91B2FD83005B986A816EE9365F1360F4")
        );
        assert!(b.front_host.is_none());
    }

    #[test]
    fn strips_torrc_bridge_prefix() {
        let b = parse_bridge_line("Bridge 1.2.3.4:8080 A7E7616C91B2FD83005B986A816EE9365F1360F4")
            .unwrap();
        assert_eq!(b.transport, Transport::Vanilla);
        assert_eq!(b.endpoint.unwrap().port, 8080);
        assert!(!b.raw.to_ascii_lowercase().starts_with("bridge "));
    }

    #[test]
    fn parses_obfs4_with_params() {
        let line = "obfs4 5.102.61.223:2133 03FB5AF28A45562076CA5520CCAF05D7F15330E8 \
                    cert=wPwENUtgpTzI+6JzuF8fUdhnJu0gEtLQTutuTOgULnFs4G1rZtY1Byg38ra0mXMjwaovZA \
                    iat-mode=0";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.transport, Transport::Obfs4);
        let ep = b.endpoint.unwrap();
        assert_eq!(ep.host, "5.102.61.223");
        assert_eq!(ep.port, 2133);
        assert_eq!(b.params.get("iat-mode").map(String::as_str), Some("0"));
        assert!(b.params.contains_key("cert"));
        assert!(b.front_host.is_none());
    }

    #[test]
    fn webtunnel_uses_url_host_and_placeholder_endpoint() {
        let line = "webtunnel [2001:db8:1d30:ff54:bba:de27:3861:ff8c]:443 \
                    0123456789ABCDEF0123456789ABCDEF01234567 \
                    url=https://rabbithole2.net/4kHLbQ ver=0.0.1";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.transport, Transport::Webtunnel);
        assert!(b.transport.is_fronted());
        let ep = b.endpoint.unwrap();
        assert!(ep.is_ipv6);
        assert!(is_placeholder_host(&ep.host));
        assert_eq!(b.front_host.as_deref(), Some("rabbithole2.net"));
    }

    #[test]
    fn snowflake_prefers_url_over_fronts() {
        let line = "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 \
                    url=https://1098762253.rsc.cdn77.org/ fronts=foursquare.com,github.githubassets.com";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.transport, Transport::Snowflake);
        assert_eq!(b.front_host.as_deref(), Some("1098762253.rsc.cdn77.org"));
    }

    #[test]
    fn meek_lite_prefers_url_over_front() {
        let line = "meek_lite 192.0.2.20:80 97700DFE9F483596DDA6264C4D7DF7641E1E39CE \
                    url=https://meek.azureedge.net/ front=ajax.aspnetcdn.com";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.transport, Transport::MeekLite);
        assert_eq!(b.front_host.as_deref(), Some("meek.azureedge.net"));
    }

    #[test]
    fn conjure_uses_registration_url_host() {
        let line = "conjure 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 \
                    url=https://registration.refraction.network/api \
                    fronts=cdn.sstatic.net,assets.cloud.censys.io transport=min";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.transport, Transport::Conjure);
        assert_eq!(
            b.front_host.as_deref(),
            Some("registration.refraction.network")
        );
        assert_eq!(b.params.get("transport").map(String::as_str), Some("min"));
    }

    #[test]
    fn dnstt_uses_doh_resolver_host() {
        let line = "dnstt 192.0.2.4:1 A998F319AABBCCDDEEFF00112233445566778899AA \
                    doh=https://dns.google/dns-query pubkey=2411deadbeef domain=t.ruhnama.net";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.transport, Transport::Dnstt);
        assert_eq!(b.front_host.as_deref(), Some("dns.google"));
    }

    #[test]
    fn dnstt_dot_strips_port() {
        let line = "dnstt 192.0.2.4:1 A998F319AABBCCDDEEFF00112233445566778899AA \
                    dot=dns.example:853 pubkey=2411deadbeef domain=t.example.net";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.front_host.as_deref(), Some("dns.example"));
    }

    #[test]
    fn fronts_only_takes_first_entry() {
        let line = "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 \
                    fronts=cdn.example.com,backup.example.com";
        let b = parse_bridge_line(line).unwrap();
        assert_eq!(b.front_host.as_deref(), Some("cdn.example.com"));
    }

    #[test]
    fn stable_id_is_whitespace_and_case_insensitive() {
        let a = parse_bridge_line(
            "obfs4 5.102.61.223:2133 03FB5AF28A45562076CA5520CCAF05D7F15330E8 cert=AbC iat-mode=0",
        )
        .unwrap();
        let b = parse_bridge_line("OBFS4   5.102.61.223:2133   03fb5af28a45562076ca5520ccaf05d7f15330e8   cert=AbC   iat-mode=0").unwrap();
        assert_eq!(a.id, b.id);
    }
}
