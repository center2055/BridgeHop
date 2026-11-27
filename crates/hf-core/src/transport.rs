//! Pluggable transport vocabulary.
//!
//! A bridge line names at most one transport. Everything HopForge decides later — whether the
//! line has a real address to probe, which sidecar binary speaks the transport, whether the
//! bridge is reached through a front — hangs off this enum, so the mapping from a feed's token
//! to a variant happens exactly once, here.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Longest transport name accepted in a bridge line. The specification imposes no limit
/// (pt-spec 3.1); a cap keeps hostile lines short.
pub const MAX_TRANSPORT_NAME_LEN: usize = 40;

/// The transports HopForge knows how to reason about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// A bare `IP:PORT FINGERPRINT` line: Tor's own TLS link, no obfuscation.
    Vanilla,
    /// IETF obfs4, spoken by the `lyrebird` sidecar.
    Obfs4,
    /// TLS + WebSocket tunnel to a reverse-proxy, spoken by the `webtunnel` sidecar.
    Webtunnel,
    /// WebRTC via volunteer proxies and a broker, spoken by the `snowflake` sidecar.
    Snowflake,
    /// Domain-fronted HTTP tunnel, spoken by the `lyrebird` sidecar (its `meeklite`
    /// transport registers under this exact name).
    MeekLite,
    /// Refraction Networking's decoy-registration transport.
    Conjure,
    /// DNS tunnel (`dnstt-client`). The bridge-line arguments are a community convention, not
    /// a Tor specification; see `crate::parse`.
    Dnstt,
    /// Any other well-formed name. Kept so nothing from a feed is silently dropped, but never
    /// dialed.
    Unknown(String),
}

/// Where a client has to connect to reach a bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialTarget {
    /// The IP:port written on the line: vanilla and obfs4.
    LineAddress,
    /// The host of the `url` argument. The address on the line is a placeholder: webtunnel.
    UrlHost,
    /// A broker, CDN front or DNS resolver reached through shared infrastructure: snowflake,
    /// meek_lite, conjure, dnstt.
    Fronted,
    /// Nothing HopForge can dial (unused by the known transports; kept so exhaustive matches
    /// over [`DialTarget`] do not need a wildcard for future ones).
    None,
}

impl Transport {
    /// Maps a bridge-line token to a transport. Accepts the spellings seen in the wild
    /// (`meek-lite`, `meek_azure`, case differences). Returns `None` for tokens that are not a
    /// legal transport name at all.
    pub fn parse(token: &str) -> Option<Transport> {
        let lower = token.to_ascii_lowercase();
        if lower.is_empty() || lower.len() > MAX_TRANSPORT_NAME_LEN {
            return None;
        }
        if !lower.starts_with(|c: char| c == '_' || c.is_ascii_lowercase()) {
            return None;
        }
        if !lower
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        Some(match lower.as_str() {
            "vanilla" => Transport::Vanilla,
            "obfs4" => Transport::Obfs4,
            "webtunnel" => Transport::Webtunnel,
            "snowflake" => Transport::Snowflake,
            "meek_lite" | "meek-lite" | "meek_azure" | "meek-azure" | "meek" => Transport::MeekLite,
            "conjure" => Transport::Conjure,
            "dnstt" => Transport::Dnstt,
            other => Transport::Unknown(other.to_string()),
        })
    }

    /// The name Tor uses for this transport in a `Bridge` / `ClientTransportPlugin` line.
    pub fn name(&self) -> Cow<'_, str> {
        match self {
            Transport::Vanilla => Cow::Borrowed("vanilla"),
            Transport::Obfs4 => Cow::Borrowed("obfs4"),
            Transport::Webtunnel => Cow::Borrowed("webtunnel"),
            Transport::Snowflake => Cow::Borrowed("snowflake"),
            Transport::MeekLite => Cow::Borrowed("meek_lite"),
            Transport::Conjure => Cow::Borrowed("conjure"),
            Transport::Dnstt => Cow::Borrowed("dnstt"),
            Transport::Unknown(name) => Cow::Borrowed(name),
        }
    }

    /// The sidecar binary that speaks this transport client-side (see `pt-sidecars/`), if one
    /// is bundled. `Dnstt` is not a managed pluggable transport: `hf-tor` runs it as a local
    /// port forwarder and points a vanilla bridge line at it.
    ///
    /// lyrebird also registers `obfs2`, `obfs3` and `scramblesuit`; HopForge keeps those as
    /// [`Transport::Unknown`] on purpose, because they are obsolete and recognized by DPI.
    pub fn sidecar(&self) -> Option<&'static str> {
        match self {
            Transport::Obfs4 | Transport::MeekLite => Some("lyrebird"),
            Transport::Snowflake => Some("snowflake"),
            Transport::Webtunnel => Some("webtunnel"),
            Transport::Conjure => Some("conjure"),
            Transport::Dnstt => Some("dnstt-client"),
            Transport::Vanilla | Transport::Unknown(_) => None,
        }
    }

    /// What a client has to reach to use this transport.
    pub fn dial_target(&self) -> DialTarget {
        match self {
            Transport::Vanilla | Transport::Obfs4 | Transport::Unknown(_) => {
                DialTarget::LineAddress
            }
            // The webtunnel client resolves the host of its `url` and dials that; the address
            // on the line is a placeholder (webtunnel v0.0.7 main/client/main.go).
            Transport::Webtunnel => DialTarget::UrlHost,
            Transport::Snowflake | Transport::MeekLite | Transport::Conjure => DialTarget::Fronted,
            Transport::Dnstt => DialTarget::Fronted,
        }
    }

    /// Whether the address on the line is part of the bridge's identity. It is not where the
    /// line carries a placeholder.
    pub fn line_addr_is_identity(&self) -> bool {
        matches!(self.dial_target(), DialTarget::LineAddress)
    }

    /// Whether HopForge can measure reachability itself, without the transport's client: by
    /// connecting to the line address, or (webtunnel) to the host of its `url`.
    pub fn is_direct(&self) -> bool {
        matches!(
            self.dial_target(),
            DialTarget::LineAddress | DialTarget::UrlHost
        )
    }

    /// Whether the bridge is reached through shared infrastructure (broker, CDN front, DNS
    /// resolver) rather than its own address. For these the address in the line is a
    /// placeholder, so connecting to it proves nothing and is never done.
    pub fn is_fronted(&self) -> bool {
        matches!(self.dial_target(), DialTarget::Fronted)
    }

    /// Whether HopForge can dial this transport at all with the binaries it ships.
    pub fn is_dialable(&self) -> bool {
        self.sidecar().is_some()
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_spellings() {
        assert_eq!(Transport::parse("obfs4"), Some(Transport::Obfs4));
        assert_eq!(Transport::parse("OBFS4"), Some(Transport::Obfs4));
        assert_eq!(Transport::parse("meek_lite"), Some(Transport::MeekLite));
        assert_eq!(Transport::parse("meek-lite"), Some(Transport::MeekLite));
        assert_eq!(Transport::parse("meek-azure"), Some(Transport::MeekLite));
        assert_eq!(Transport::parse("snowflake"), Some(Transport::Snowflake));
        assert_eq!(Transport::parse("vanilla"), Some(Transport::Vanilla));
    }

    #[test]
    fn unknown_but_well_formed_is_kept() {
        assert_eq!(
            Transport::parse("obfs3"),
            Some(Transport::Unknown("obfs3".into()))
        );
        assert_eq!(
            Transport::parse("scramblesuit"),
            Some(Transport::Unknown("scramblesuit".into()))
        );
    }

    #[test]
    fn malformed_names_are_rejected() {
        for bad in [
            "",
            "9obfs4",
            "obfs4 x",
            "a.b",
            "x=y",
            "[::1]:80",
            "1.2.3.4:443",
        ] {
            assert!(Transport::parse(bad).is_none(), "{bad:?} must not parse");
        }
        let long = "a".repeat(MAX_TRANSPORT_NAME_LEN + 1);
        assert!(Transport::parse(&long).is_none());
    }

    #[test]
    fn name_round_trips_through_the_canonical_spelling() {
        for t in [
            Transport::Vanilla,
            Transport::Obfs4,
            Transport::Webtunnel,
            Transport::Snowflake,
            Transport::MeekLite,
            Transport::Conjure,
            Transport::Dnstt,
        ] {
            assert_eq!(Transport::parse(&t.name()), Some(t.clone()));
        }
    }

    #[test]
    fn sidecars_and_shape() {
        assert_eq!(Transport::Obfs4.sidecar(), Some("lyrebird"));
        assert_eq!(Transport::MeekLite.sidecar(), Some("lyrebird"));
        assert!(Transport::Obfs4.is_direct() && !Transport::Obfs4.is_fronted());
        assert!(Transport::Webtunnel.is_direct() && !Transport::Webtunnel.line_addr_is_identity());
        assert!(Transport::Snowflake.is_fronted() && !Transport::Snowflake.is_direct());
        assert!(!Transport::Unknown("obfs3".into()).is_dialable());
    }

    #[test]
    fn serde_uses_tor_spellings() {
        assert_eq!(
            serde_json::to_string(&Transport::MeekLite).unwrap(),
            "\"meek_lite\""
        );
        assert_eq!(
            serde_json::from_str::<Transport>("\"obfs4\"").unwrap(),
            Transport::Obfs4
        );
    }
}
