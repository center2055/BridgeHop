//! Built-in default bridge lines for transports the collector does not host.
//!
//! These mirror the bridges shipped with Tor Browser. The listed IP is a documentation
//! placeholder; the transport reaches Tor via a broker / domain fronting, so the scanner probes
//! the broker/front host. Ported verbatim from OnionHop's `BridgeSourceService.BuiltInBridges`.

pub const SNOWFLAKE: &[&str] = &[
    "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://1098762253.rsc.cdn77.org/ fronts=www.cdn77.com,www.phpmyadmin.net ice=stun:stun.l.google.com:19302,stun:stun.antisip.com:3478,stun:stun.bluesip.net:3478,stun:stun.dus.net:3478,stun:stun.epygi.com:3478 utls-imitate=hellorandomizedalpn",
    "snowflake 192.0.2.4:80 8838024498816A039FCBBAB14E6F40A0843051FA fingerprint=8838024498816A039FCBBAB14E6F40A0843051FA url=https://1098762253.rsc.cdn77.org/ fronts=www.cdn77.com,www.phpmyadmin.net ice=stun:stun.l.google.com:19302,stun:stun.antisip.com:3478,stun:stun.bluesip.net:3478,stun:stun.dus.net:3478,stun:stun.epygi.com:3478 utls-imitate=hellorandomizedalpn",
];

pub const MEEK_AZURE: &[&str] = &[
    "meek_lite 192.0.2.20:80 97700DFE9F483596DDA6264C4D7DF7641E1E39CE url=https://meek.azureedge.net/ front=ajax.aspnetcdn.com",
];

pub const CONJURE: &[&str] = &[
    "conjure 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://registration.refraction.network/api fronts=cdn.sstatic.net,assets.cloud.censys.io transport=min",
];

pub const DNSTT: &[&str] = &[
    "dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE doh=https://dns.google/dns-query pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f domain=t.ruhnama.net",
    "dnstt 192.0.2.4:2 80EEFA4F4875ED2B7B5A86DF2D7588AD32E29F15 doh=https://dns.google/dns-query pubkey=a2fb71077eeaa54a02cda7a90be306af5d299ab21822a8b277d4eacbc9168631 domain=t2.bypasscensorship.org",
    "dnstt 192.0.2.4:3 74D409BED3E2F881F365543A72C8F079CB84FFEB doh=https://dns.google/dns-query pubkey=c596c458fc3453dc40903ab235f5854a2609831075640c4c5584f76de05b8271 domain=t.arkadag.org",
];

/// All built-in fronted-transport pools, in a stable order.
pub const ALL: &[(&str, &[&str])] = &[
    ("snowflake", SNOWFLAKE),
    ("meek-azure", MEEK_AZURE),
    ("conjure", CONJURE),
    ("dnstt", DNSTT),
];

/// Built-in lines for a transport token, if any.
pub fn built_in(transport: &str) -> Option<&'static [&'static str]> {
    match transport {
        "snowflake" => Some(SNOWFLAKE),
        "meek-azure" | "meek_azure" | "meek_lite" | "meek" => Some(MEEK_AZURE),
        "conjure" => Some(CONJURE),
        "dnstt" => Some(DNSTT),
        _ => None,
    }
}
