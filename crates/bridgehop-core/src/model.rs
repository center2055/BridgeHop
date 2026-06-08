//! Core data types: transports, bridges, and scan results.
//!
//! These are deliberately UI-agnostic and serialize cleanly to JSON for the Tauri
//! front end, the CLI, and on-disk storage.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A reachable-but-slow handshake takes at least this many milliseconds.
///
/// Ported from OnionHop's `BridgeScanService` (`SlowThresholdMs = 500`).
pub const SLOW_THRESHOLD_MS: u64 = 500;

/// A pluggable-transport type. Unknown tokens are preserved verbatim so nothing is lost.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Transport {
    Vanilla,
    Obfs4,
    Obfs3,
    Obfs2,
    ScrambleSuit,
    Webtunnel,
    Snowflake,
    MeekLite,
    MeekAzure,
    Meek,
    Conjure,
    Dnstt,
    /// An unrecognized transport token, kept as-is.
    Unknown(String),
}

impl Transport {
    /// The canonical lowercase token for this transport (e.g. `"obfs4"`).
    pub fn token(&self) -> &str {
        match self {
            Transport::Vanilla => "vanilla",
            Transport::Obfs4 => "obfs4",
            Transport::Obfs3 => "obfs3",
            Transport::Obfs2 => "obfs2",
            Transport::ScrambleSuit => "scramblesuit",
            Transport::Webtunnel => "webtunnel",
            Transport::Snowflake => "snowflake",
            Transport::MeekLite => "meek_lite",
            Transport::MeekAzure => "meek-azure",
            Transport::Meek => "meek",
            Transport::Conjure => "conjure",
            Transport::Dnstt => "dnstt",
            Transport::Unknown(s) => s,
        }
    }

    /// `true` for transports that do not connect to their bridge line's IP:port directly,
    /// but instead reach a broker / front host (probe that host on `:443` instead).
    pub fn is_fronted(&self) -> bool {
        matches!(
            self,
            Transport::Webtunnel
                | Transport::Snowflake
                | Transport::Meek
                | Transport::MeekLite
                | Transport::MeekAzure
                | Transport::Conjure
                | Transport::Dnstt
        )
    }

    /// `true` if this is a recognized transport (not [`Transport::Unknown`]).
    pub fn is_known(&self) -> bool {
        !matches!(self, Transport::Unknown(_))
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

impl FromStr for Transport {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let norm = s.trim().to_ascii_lowercase();
        Ok(match norm.as_str() {
            "vanilla" => Transport::Vanilla,
            "obfs4" => Transport::Obfs4,
            "obfs3" => Transport::Obfs3,
            "obfs2" => Transport::Obfs2,
            "scramblesuit" => Transport::ScrambleSuit,
            "webtunnel" => Transport::Webtunnel,
            "snowflake" => Transport::Snowflake,
            "meek_lite" | "meeklite" => Transport::MeekLite,
            "meek-azure" | "meek_azure" | "meekazure" => Transport::MeekAzure,
            "meek" => Transport::Meek,
            "conjure" => Transport::Conjure,
            "dnstt" => Transport::Dnstt,
            "" => Transport::Vanilla,
            _ => Transport::Unknown(norm),
        })
    }
}

impl Serialize for Transport {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.token())
    }
}

impl<'de> Deserialize<'de> for Transport {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(s.parse().expect("Transport::from_str is infallible"))
    }
}

/// A resolved `host:port` endpoint extracted from a bridge line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub is_ipv6: bool,
}

/// A fully parsed bridge line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bridge {
    /// The original (normalized) bridge line.
    pub raw: String,
    pub transport: Transport,
    /// The literal IP:port from the line, if present. For fronted transports this is often a
    /// documentation placeholder (RFC 5737 / RFC 3849); the real target is [`Bridge::front_host`].
    pub endpoint: Option<Endpoint>,
    /// The 40-hex relay fingerprint, if present.
    pub fingerprint: Option<String>,
    /// For fronted transports, the broker / front host to probe instead of `endpoint`.
    pub front_host: Option<String>,
    /// Remaining `key=value` parameters (cert, iat-mode, url, domain, …).
    pub params: BTreeMap<String, String>,
    /// Stable identifier for dedupe and storage (derived from the normalized line).
    pub id: String,
}

/// The outcome of probing a single bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reachability {
    /// Handshake succeeded under [`SLOW_THRESHOLD_MS`].
    Reachable,
    /// Handshake succeeded but was slow.
    Slow,
    /// The endpoint could not be reached.
    Unreachable,
    /// The line could not be parsed into a probe target.
    Unparsed,
    /// A fronted transport whose broker / front host responded.
    Fronted,
}

impl Reachability {
    /// Lowercase token used for storage and display.
    pub fn as_str(self) -> &'static str {
        match self {
            Reachability::Reachable => "reachable",
            Reachability::Slow => "slow",
            Reachability::Unreachable => "unreachable",
            Reachability::Unparsed => "unparsed",
            Reachability::Fronted => "fronted",
        }
    }

    /// Whether this outcome counts as a usable bridge.
    pub fn is_working(self) -> bool {
        matches!(
            self,
            Reachability::Reachable | Reachability::Slow | Reachability::Fronted
        )
    }
}

/// Result of an optional deep (real pluggable-transport handshake) verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeepResult {
    pub ok: bool,
    /// Which transport client performed the verification.
    pub method: String,
    /// Round-trip time of the SOCKS CONNECT through the transport, if measured.
    pub socks_ms: Option<u32>,
    pub detail: String,
}

/// Geo / ASN information for a probed address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GeoInfo {
    pub country: Option<String>,
    pub asn: Option<u32>,
    pub as_org: Option<String>,
}

/// The full result of scanning one bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub bridge_id: String,
    pub raw: String,
    pub transport: Transport,
    /// The host actually probed (the endpoint host, or the front host for fronted transports).
    pub probed_host: String,
    pub probed_port: u16,
    pub ping_ms: Option<u32>,
    pub reachability: Reachability,
    pub detail: String,
    pub deep: Option<DeepResult>,
    pub geo: Option<GeoInfo>,
}

impl ScanResult {
    /// `true` if the bridge is usable: reachable, slow-but-reachable, or a responding front.
    pub fn is_working(&self) -> bool {
        self.reachability.is_working()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_roundtrips_through_token() {
        for t in [
            Transport::Vanilla,
            Transport::Obfs4,
            Transport::Webtunnel,
            Transport::Snowflake,
            Transport::MeekLite,
            Transport::MeekAzure,
            Transport::Conjure,
            Transport::Dnstt,
        ] {
            let token = t.token().to_string();
            assert_eq!(token.parse::<Transport>().unwrap(), t);
        }
    }

    #[test]
    fn unknown_transport_is_preserved() {
        let t: Transport = "FancyNewPT".parse().unwrap();
        assert_eq!(t, Transport::Unknown("fancynewpt".to_string()));
        assert!(!t.is_known());
    }

    #[test]
    fn fronted_classification_matches_plan() {
        assert!(Transport::Webtunnel.is_fronted());
        assert!(Transport::Snowflake.is_fronted());
        assert!(!Transport::Obfs4.is_fronted());
        assert!(!Transport::Vanilla.is_fronted());
    }
}
