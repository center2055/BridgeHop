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
    parse_bridge_lines(text.lines())
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

    #[cfg(feature = "qr")]
    #[test]
    fn qr_svg_produces_svg() {
        let svg = qr_svg(LINE).unwrap();
        assert!(svg.contains("<svg"));
    }
}
