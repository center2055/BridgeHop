//! Import and export of bridge lines in plain, torrc, and JSON formats, plus QR encoding.
//!
//! Every import funnels through the same parser as the rest of the app, so torrc `Bridge `
//! prefixes, comments, BOMs, and duplicates are handled consistently.

use serde::{Deserialize, Serialize};

use crate::model::Bridge;
use crate::parse::{parse_bridge_line, parse_bridge_lines};

/// Output format for [`export`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// One bridge line per row.
    Plain,
    /// One `Bridge <line>` per row (torrc form).
    Torrc,
    /// A JSON object `{ "bridges": [ ... ] }` with full parsed metadata.
    Json,
}

#[derive(Serialize)]
struct ExportEnvelope<'a> {
    bridges: &'a [Bridge],
}

#[derive(Deserialize)]
struct ImportEnvelope {
    bridges: Vec<Bridge>,
}

/// Render bridge lines in the requested format.
pub fn export(lines: &[String], format: ExportFormat) -> String {
    match format {
        ExportFormat::Plain => lines.join("\n"),
        ExportFormat::Torrc => lines
            .iter()
            .map(|line| format!("Bridge {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        ExportFormat::Json => {
            let bridges: Vec<Bridge> = lines.iter().filter_map(|l| parse_bridge_line(l)).collect();
            serde_json::to_string_pretty(&ExportEnvelope { bridges: &bridges }).unwrap_or_default()
        }
    }
}

/// Parse bridges from arbitrary imported text: JSON (from a previous export) or plain/torrc lines.
pub fn import(text: &str) -> Vec<Bridge> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Some(raws) = import_json(trimmed) {
            return parse_bridge_lines(raws.iter().map(String::as_str));
        }
    }
    // Normalize line endings before splitting. `str::lines()` only splits on `\n`/`\r\n`, so a
    // file saved with bare CR (or mixed) endings would collapse many bridges into a single line
    // and only the first would be parsed — exactly the "only a few got scanned" symptom.
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    parse_bridge_lines(normalized.lines())
}

fn import_json(text: &str) -> Option<Vec<String>> {
    if let Ok(env) = serde_json::from_str::<ImportEnvelope>(text) {
        return Some(env.bridges.into_iter().map(|b| b.raw).collect());
    }
    if let Ok(bridges) = serde_json::from_str::<Vec<Bridge>>(text) {
        return Some(bridges.into_iter().map(|b| b.raw).collect());
    }
    if let Ok(raws) = serde_json::from_str::<Vec<String>>(text) {
        return Some(raws);
    }
    None
}

/// Render `text` as a scalable QR code (SVG string). Fails if the text is too large for a QR code.
#[cfg(feature = "qr")]
pub fn qr_svg(text: &str) -> crate::Result<String> {
    use qrcode::render::svg;
    use qrcode::QrCode;

    let code = QrCode::new(text.as_bytes())
        .map_err(|e| crate::Error::Other(format!("could not build QR code: {e}")))?;
    let image = code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build();
    Ok(image)
}

/// Encode a bridge line as a SlipNet `slipnet://` config URI for import into SlipNet
/// (<https://github.com/anonvector/SlipNet>). Returns `None` if the line doesn't parse.
///
/// SlipNet's "Tor" profiles are the `snowflake` tunnel type carrying the bridge in a
/// `torBridgeLines` field; transport is auto-detected from the line prefix. The wire format is
/// `slipnet://` + base64 of a `28|`-pipe-delimited v28 profile (mirrors SlipNet's `ConfigExporter`).
/// SlipNet's importer requires a non-blank domain and at least one resolver even for Tor profiles,
/// so inert placeholders are supplied; the Tor tunnel ignores them at runtime.
pub fn to_slipnet_uri(line: &str) -> Option<String> {
    let bridge = parse_bridge_line(line)?;
    Some(build_slipnet_uri(&bridge))
}

fn build_slipnet_uri(bridge: &Bridge) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    // "|" delimits fields, so strip it from any free-text values.
    let sanitize = |s: &str| s.replace('|', "");
    let name = match &bridge.endpoint {
        Some(ep) => sanitize(&format!("{} {}", bridge.transport.token(), ep.host)),
        None => sanitize(&format!("{} bridge", bridge.transport.token())),
    };
    let bridge_b64 = STANDARD.encode(bridge.raw.as_bytes());

    // SlipNet ConfigExporter v28 field order. Only name (3), torBridgeLines (27), and the inert
    // domain/resolver placeholders (4/5) vary; everything else is the ServerProfile default.
    let fields: Vec<String> = vec![
        "28".into(),           // 1  version
        "snowflake".into(),    // 2  tunnelType (displayed as "Tor")
        name,                  // 3  name
        "tor".into(),          // 4  domain (placeholder; ignored for Tor)
        "1.1.1.1:53:0".into(), // 5  resolvers host:port:auth (placeholder; ignored for Tor)
        "0".into(),            // 6  authoritativeMode
        "5000".into(),         // 7  keepAliveInterval
        "bbr".into(),          // 8  congestionControl
        "1080".into(),         // 9  tcpListenPort
        "127.0.0.1".into(),    // 10 tcpListenHost
        "0".into(),            // 11 gsoEnabled
        "".into(),             // 12 dnsttPublicKey
        "".into(),             // 13 socksUsername
        "".into(),             // 14 socksPassword
        "0".into(),            // 15 sshEnabled
        "".into(),             // 16 sshUsername
        "".into(),             // 17 sshPassword
        "22".into(),           // 18 sshPort
        "0".into(),            // 19 (reserved)
        "127.0.0.1".into(),    // 20 sshHost
        "0".into(),            // 21 (reserved: useServerDns)
        "".into(),             // 22 dohUrl
        "udp".into(),          // 23 dnsTransport
        "password".into(),     // 24 sshAuthType
        "".into(),             // 25 sshPrivateKey (b64)
        "".into(),             // 26 sshKeyPassphrase (b64)
        bridge_b64,            // 27 torBridgeLines (b64) — the bridge
        "0".into(),            // 28 dnsttAuthoritative
        "443".into(),          // 29 naivePort
        "".into(),             // 30 naiveUsername
        "".into(),             // 31 naivePassword (b64)
        "0".into(),            // 32 isLocked
        "".into(),             // 33 lockPasswordHash
        "0".into(),            // 34 expirationDate
        "0".into(),            // 35 allowSharing
        "".into(),             // 36 boundDeviceId
        "0".into(),            // 37 hideResolvers
        "".into(),             // 38 hiddenResolvers
        "0".into(),            // 39 noizdnsStealth
        "0".into(),            // 40 dnsPayloadSize
        "1080".into(),         // 41 socks5ServerPort
        "0".into(),            // 42 vaydnsDnsttCompat
        "txt".into(),          // 43 vaydnsRecordType
        "101".into(),          // 44 vaydnsMaxQnameLen
        "0.0".into(),          // 45 vaydnsRps
        "0".into(),            // 46 vaydnsIdleTimeout
        "0".into(),            // 47 vaydnsKeepalive
        "0".into(),            // 48 vaydnsUdpTimeout
        "0".into(),            // 49 vaydnsMaxNumLabels
        "0".into(),            // 50 vaydnsClientIdSize
        "0".into(),            // 51 sshTlsEnabled
        "".into(),             // 52 sshTlsSni
        "".into(),             // 53 sshHttpProxyHost
        "8080".into(),         // 54 sshHttpProxyPort
        "".into(),             // 55 sshHttpProxyCustomHost
        "0".into(),            // 56 sshWsEnabled
        "/".into(),            // 57 sshWsPath
        "1".into(),            // 58 sshWsUseTls
        "".into(),             // 59 sshWsCustomHost
        "".into(),             // 60 sshPayload (b64)
        "roundrobin".into(),   // 61 resolverMode
        "3".into(),            // 62 rrSpreadCount
        "".into(),             // 63 vlessUuid
        "tls".into(),          // 64 vlessSecurity
        "ws".into(),           // 65 vlessTransport
        "/".into(),            // 66 vlessWsPath
        "".into(),             // 67 cdnIp
        "443".into(),          // 68 cdnPort
        "1".into(),            // 69 sniFragmentEnabled
        "micro".into(),        // 70 sniFragmentStrategy
        "300".into(),          // 71 sniFragmentDelayMs
        "".into(),             // 72 (reserved: legacy SNI)
        "0".into(),            // 73 chPaddingEnabled
        "1".into(),            // 74 wsHeaderObfuscation
        "0".into(),            // 75 wsPaddingEnabled
        "8".into(),            // 76 sniSpoofTtl
        "".into(),             // 77 fakeDecoyHost
        "0".into(),            // 78 tcpMaxSeg
        "".into(),             // 79 vlessSni
    ];
    format!("slipnet://{}", STANDARD.encode(fields.join("|").as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str =
        "obfs4 1.2.3.4:443 A7E7616C91B2FD83005B986A816EE9365F1360F4 cert=abc iat-mode=0";

    #[test]
    fn plain_and_torrc_roundtrip() {
        let lines = vec![LINE.to_string()];
        let torrc = export(&lines, ExportFormat::Torrc);
        assert_eq!(torrc, format!("Bridge {LINE}"));
        // Importing the torrc form yields the same bridge back (prefix stripped).
        let back = import(&torrc);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].raw, LINE);
    }

    #[test]
    fn json_export_imports_back() {
        let lines = vec![LINE.to_string()];
        let json = export(&lines, ExportFormat::Json);
        assert!(json.contains("\"bridges\""));
        let back = import(&json);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].raw, LINE);
    }

    #[test]
    fn import_plain_lines_with_comments() {
        let text = "# a list\n1.2.3.4:443 A7E7616C91B2FD83005B986A816EE9365F1360F4\n\n";
        let back = import(text);
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn import_handles_cr_and_crlf_line_endings() {
        let fp = "A7E7616C91B2FD83005B986A816EE9365F1360F4";
        // Bare CR (old/odd editors) and CRLF must both split into separate bridges, not collapse.
        let cr = format!("1.1.1.1:443 {fp}\r2.2.2.2:443 {fp}\r3.3.3.3:443 {fp}");
        assert_eq!(import(&cr).len(), 3);
        let crlf = format!("1.1.1.1:443 {fp}\r\n2.2.2.2:443 {fp}\r\n3.3.3.3:443 {fp}");
        assert_eq!(import(&crlf).len(), 3);
    }

    #[test]
    fn slipnet_uri_has_v28_tor_layout() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine as _;

        let uri = to_slipnet_uri(LINE).unwrap();
        assert!(uri.starts_with("slipnet://"));
        let decoded =
            String::from_utf8(STANDARD.decode(&uri["slipnet://".len()..]).unwrap()).unwrap();
        let fields: Vec<&str> = decoded.split('|').collect();
        assert_eq!(fields.len(), 79, "v28 profile must have 79 fields");
        assert_eq!(fields[0], "28");
        assert_eq!(fields[1], "snowflake"); // SlipNet's Tor tunnel type
        assert!(
            !fields[3].is_empty(),
            "importer requires a non-blank domain"
        );
        assert!(!fields[4].is_empty(), "importer requires a resolver");
        // Field 27 (index 26) carries the base64 bridge line.
        let back = String::from_utf8(STANDARD.decode(fields[26]).unwrap()).unwrap();
        assert_eq!(back, LINE);
    }

    #[cfg(feature = "qr")]
    #[test]
    fn qr_svg_produces_svg() {
        let svg = qr_svg(LINE).unwrap();
        assert!(svg.contains("<svg"));
    }
}
