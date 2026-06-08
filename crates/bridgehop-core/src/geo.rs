//! Optional geo / ASN lookups via MaxMind GeoLite2 databases.
//!
//! GeoLite2 databases require a (free) MaxMind account and are not redistributable, so BridgeHop
//! does not bundle them. Drop `GeoLite2-Country.mmdb` and/or `GeoLite2-ASN.mmdb` into the geo
//! directory ([`GeoDb::geo_dir`]) and lookups light up automatically. Everything degrades
//! gracefully to "no data" when a database is absent.

use std::net::IpAddr;
use std::path::PathBuf;

use maxminddb::{geoip2, Reader};

use crate::model::{GeoInfo, ScanResult};
use crate::paths;

/// Loaded GeoLite2 databases (either may be absent).
pub struct GeoDb {
    country: Option<Reader<Vec<u8>>>,
    asn: Option<Reader<Vec<u8>>>,
}

impl GeoDb {
    /// Directory BridgeHop looks in for GeoLite2 `.mmdb` files.
    pub fn geo_dir() -> PathBuf {
        paths::data_dir().join("geo")
    }

    /// Open whichever GeoLite2 databases are present in [`GeoDb::geo_dir`].
    pub fn open() -> GeoDb {
        let dir = Self::geo_dir();
        GeoDb {
            country: Reader::open_readfile(dir.join("GeoLite2-Country.mmdb")).ok(),
            asn: Reader::open_readfile(dir.join("GeoLite2-ASN.mmdb")).ok(),
        }
    }

    /// An empty database (no lookups succeed).
    pub fn empty() -> GeoDb {
        GeoDb {
            country: None,
            asn: None,
        }
    }

    /// Whether any database is loaded.
    pub fn is_available(&self) -> bool {
        self.country.is_some() || self.asn.is_some()
    }

    /// Look up country and ASN information for an IP address.
    pub fn lookup(&self, ip: IpAddr) -> GeoInfo {
        let mut info = GeoInfo::default();

        if let Some(reader) = &self.country {
            let result: Result<geoip2::Country, _> = reader.lookup(ip);
            if let Ok(record) = result {
                info.country = record
                    .country
                    .and_then(|c| c.iso_code)
                    .map(|code| code.to_string());
            }
        }

        if let Some(reader) = &self.asn {
            let result: Result<geoip2::Asn, _> = reader.lookup(ip);
            if let Ok(record) = result {
                info.asn = record.autonomous_system_number;
                info.as_org = record
                    .autonomous_system_organization
                    .map(|org| org.to_string());
            }
        }

        info
    }
}

/// Enrich scan results in place with geo info for IP-literal probed hosts. No-op when no database
/// is loaded or a probed host is not an IP literal (e.g. a fronted broker hostname).
pub fn enrich(results: &mut [ScanResult], db: &GeoDb) {
    if !db.is_available() {
        return;
    }
    for result in results.iter_mut() {
        if let Ok(ip) = result.probed_host.parse::<IpAddr>() {
            result.geo = Some(db.lookup(ip));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Reachability, ScanResult, Transport};

    fn result(host: &str) -> ScanResult {
        ScanResult {
            bridge_id: "id".to_string(),
            raw: "raw".to_string(),
            transport: Transport::Vanilla,
            probed_host: host.to_string(),
            probed_port: 443,
            ping_ms: Some(10),
            reachability: Reachability::Reachable,
            detail: String::new(),
            deep: None,
            geo: None,
        }
    }

    #[test]
    fn empty_db_is_unavailable_and_enrich_is_noop() {
        let db = GeoDb::empty();
        assert!(!db.is_available());
        let mut results = vec![result("1.2.3.4"), result("example.com")];
        enrich(&mut results, &db);
        assert!(results.iter().all(|r| r.geo.is_none()));
    }
}
