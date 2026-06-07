//! Detection of documentation / placeholder IP addresses that never route.
//!
//! Fronted and webtunnel bridge lines carry a filler IP:port (RFC 5737 for IPv4,
//! RFC 3849 `2001:db8::/32` for IPv6); their real endpoint is a URL / broker host.
//! Ported from OnionHop's `BridgeScanService.IsPlaceholderHost`.

const PLACEHOLDER_PREFIXES: &[&str] = &[
    "192.0.2.",
    "198.51.100.",
    "203.0.113.",
    "0.0.0.0",
    "2001:db8:",
    "2001:0db8:",
];

/// Returns `true` if `host` is a documentation/placeholder address that never routes.
pub fn is_placeholder_host(host: &str) -> bool {
    if host.trim().is_empty() {
        return false;
    }

    for prefix in PLACEHOLDER_PREFIXES {
        // IPv6 hex may be upper- or lower-case; compare case-insensitively (harmless for IPv4).
        if host
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            return true;
        }
    }

    host == "::" || host == "::1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_documentation_ranges() {
        assert!(is_placeholder_host("192.0.2.20"));
        assert!(is_placeholder_host("198.51.100.5"));
        assert!(is_placeholder_host("203.0.113.9"));
        assert!(is_placeholder_host("0.0.0.0"));
        assert!(is_placeholder_host("2001:db8:1d30:ff54:bba:de27:3861:ff8c"));
        assert!(is_placeholder_host("2001:0DB8::1"));
        assert!(is_placeholder_host("::"));
        assert!(is_placeholder_host("::1"));
    }

    #[test]
    fn allows_real_addresses() {
        assert!(!is_placeholder_host("5.102.61.223"));
        assert!(!is_placeholder_host("2606:4700:4700::1111"));
        assert!(!is_placeholder_host(""));
        assert!(!is_placeholder_host("   "));
    }
}
