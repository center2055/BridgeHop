//! Bridge sources: the OnionHop community collector (with mirror fallback) plus built-in
//! defaults for fronted transports the collector does not host.
//!
//! Ported from OnionHop's `BridgeSourceService`: GitHub raw is tried first, then GitHub Pages;
//! the first mirror that returns a non-empty list wins. `all` aggregates every collector
//! transport plus the built-in pools.

pub mod builtin;
mod cache;

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Collector mirror base URLs (must end with `/`). All are raced in parallel (see
/// `fetch_collector`), so order doesn't imply priority — the fastest reachable one wins. The
/// GitHub origins plus the jsDelivr and Statically CDNs serve identical files over different
/// infrastructure, so at least one is usually reachable through regional blocks/throttling.
pub const MIRROR_BASES: &[&str] = &[
    "https://raw.githubusercontent.com/center2055/OnionHop-Bridges-Collector/main/bridge/",
    "https://center2055.github.io/OnionHop-Bridges-Collector/bridge/",
    "https://cdn.jsdelivr.net/gh/center2055/OnionHop-Bridges-Collector@main/bridge/",
    "https://cdn.statically.io/gh/center2055/OnionHop-Bridges-Collector@main/bridge/",
];

/// Transports the collector publishes curated, region-tested files for.
pub const COLLECTOR_TRANSPORTS: &[&str] = &["obfs4", "webtunnel", "vanilla"];

/// Transports offered as source choices in the UI/CLI (`all` aggregates everything).
pub const SOURCE_TRANSPORTS: &[&str] = &[
    "all",
    "obfs4",
    "webtunnel",
    "vanilla",
    "snowflake",
    "meek-azure",
    "conjure",
    "dnstt",
];

/// Curation category of a collector list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Region-tested and recently active (`_tested`).
    Tested,
    /// Seen in the last 72 hours (`_72h`).
    Fresh72h,
    /// The complete archive (no suffix).
    FullArchive,
}

impl Category {
    /// Collector file-name suffix for this category.
    pub fn suffix(self) -> &'static str {
        match self {
            Category::Tested => "_tested",
            Category::Fresh72h => "_72h",
            Category::FullArchive => "",
        }
    }
}

/// What to fetch: a transport, a category, and an IP version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selection {
    pub transport: String,
    pub category: Category,
    pub ipv6: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            transport: "all".to_string(),
            category: Category::Tested,
            ipv6: false,
        }
    }
}

/// The outcome of a fetch: the bridge lines, a human-readable source label, and whether the
/// lines came from a stale cache (network unavailable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub lines: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub stale: bool,
}

/// Build a shared HTTP client (rustls, sensible timeout, BridgeHop user agent).
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("BridgeHop/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(25))
        .build()
        .expect("failed to build HTTP client")
}

/// Collector file name for a transport/category/IP-version combination, e.g.
/// `obfs4.txt`, `obfs4_ipv6.txt`, `obfs4_72h.txt`, `obfs4_ipv6_tested.txt`.
pub fn build_file_name(transport: &str, category: Category, ipv6: bool) -> String {
    let transport = transport.trim().to_ascii_lowercase();
    let ipv6 = if ipv6 { "_ipv6" } else { "" };
    format!("{transport}{ipv6}{}.txt", category.suffix())
}

/// Fetch bridges for the given selection, trying built-ins and collector mirrors as appropriate.
pub async fn fetch(selection: &Selection, client: &reqwest::Client) -> Result<FetchResult> {
    let transport = selection.transport.trim().to_ascii_lowercase();

    if transport == "all" {
        return fetch_all(selection.category, selection.ipv6, client).await;
    }

    // Transports the collector does not host use built-in defaults.
    if !COLLECTOR_TRANSPORTS.contains(&transport.as_str()) {
        if let Some(lines) = builtin::built_in(&transport) {
            return Ok(FetchResult {
                lines: lines.iter().map(|s| s.to_string()).collect(),
                source: format!("built-in:{transport}"),
                stale: false,
            });
        }
    }

    fetch_collector(&transport, selection.category, selection.ipv6, client).await
}

/// Race every mirror in parallel; the first to return a non-empty list wins. Racing (rather than
/// trying mirrors one after another) is what makes fetching work on censored networks: which
/// mirrors are reachable varies, and a blocked or slow mirror would otherwise hang the whole fetch
/// until it timed out before the reachable one was even tried.
async fn fetch_collector(
    transport: &str,
    category: Category,
    ipv6: bool,
    client: &reqwest::Client,
) -> Result<FetchResult> {
    let file = build_file_name(transport, category, ipv6);
    let mut set = tokio::task::JoinSet::new();
    for base in MIRROR_BASES {
        let url = format!("{base}{file}");
        let client = client.clone();
        set.spawn(async move {
            let response = client.get(&url).send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            let lines = parse_lines(&response.text().await.ok()?);
            (!lines.is_empty()).then_some((lines, url))
        });
    }
    // join_next yields in completion order, so this is the fastest reachable mirror with content.
    while let Some(joined) = set.join_next().await {
        if let Ok(Some((lines, source))) = joined {
            set.abort_all();
            return Ok(FetchResult {
                lines,
                source,
                stale: false,
            });
        }
    }
    Err(Error::Network(format!(
        "no collector mirror returned bridges for {transport}"
    )))
}

/// Aggregate every collector transport for the chosen category/IP version, plus all built-ins.
async fn fetch_all(
    category: Category,
    ipv6: bool,
    client: &reqwest::Client,
) -> Result<FetchResult> {
    let mut aggregate: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();

    for transport in COLLECTOR_TRANSPORTS {
        if let Ok(result) = fetch_collector(transport, category, ipv6, client).await {
            aggregate.extend(result.lines);
            sources.push((*transport).to_string());
        }
    }

    for (name, lines) in builtin::ALL {
        aggregate.extend(lines.iter().map(|s| s.to_string()));
        sources.push((*name).to_string());
    }

    let deduped = dedupe_case_insensitive(aggregate);
    if deduped.is_empty() {
        return Err(Error::Network(
            "no bridges available from any source".to_string(),
        ));
    }

    Ok(FetchResult {
        lines: deduped,
        source: format!("all:{}", sources.join("+")),
        stale: false,
    })
}

/// Fetch with an on-disk cache: a successful fetch refreshes the cache; a failed fetch falls back
/// to the most recent cached lines (marked `stale`) so bridges remain available offline.
pub async fn fetch_with_cache(
    selection: &Selection,
    client: &reqwest::Client,
) -> Result<FetchResult> {
    let key = cache_key(selection);
    match fetch(selection, client).await {
        Ok(result) => {
            cache::put(&key, &result.lines);
            Ok(result)
        }
        Err(err) => match cache::get(&key) {
            Some(entry) if !entry.lines.is_empty() => Ok(FetchResult {
                lines: entry.lines,
                source: format!("cache:{}", selection.transport),
                stale: true,
            }),
            _ => Err(err),
        },
    }
}

/// Stable cache key for a selection.
fn cache_key(selection: &Selection) -> String {
    let category = match selection.category {
        Category::Tested => "tested",
        Category::Fresh72h => "fresh72h",
        Category::FullArchive => "full",
    };
    format!(
        "{}|{}|{}",
        selection.transport.trim().to_ascii_lowercase(),
        category,
        selection.ipv6
    )
}

/// Split fetched text into trimmed, comment-free, case-insensitively-deduplicated lines.
pub fn parse_lines(content: &str) -> Vec<String> {
    let lines = content
        .replace("\r\n", "\n")
        .split('\n')
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    dedupe_case_insensitive(lines)
}

/// Order-preserving, case-insensitive de-duplication.
fn dedupe_case_insensitive(lines: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    lines
        .into_iter()
        .filter(|line| seen.insert(line.to_ascii_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_match_collector_layout() {
        assert_eq!(
            build_file_name("obfs4", Category::FullArchive, false),
            "obfs4.txt"
        );
        assert_eq!(
            build_file_name("obfs4", Category::FullArchive, true),
            "obfs4_ipv6.txt"
        );
        assert_eq!(
            build_file_name("obfs4", Category::Fresh72h, false),
            "obfs4_72h.txt"
        );
        assert_eq!(
            build_file_name("obfs4", Category::Tested, true),
            "obfs4_ipv6_tested.txt"
        );
        assert_eq!(
            build_file_name("WebTunnel", Category::Tested, false),
            "webtunnel_tested.txt"
        );
    }

    #[test]
    fn parse_lines_trims_dedupes_and_drops_comments() {
        let content =
            "# header\r\nobfs4 1.2.3.4:1\r\n\r\nOBFS4 1.2.3.4:1\n   \n# note\nvanilla 5.6.7.8:9\n";
        let lines = parse_lines(content);
        assert_eq!(
            lines,
            vec![
                "obfs4 1.2.3.4:1".to_string(),
                "vanilla 5.6.7.8:9".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn builtin_transport_resolves_without_network() {
        let client = http_client();
        let selection = Selection {
            transport: "snowflake".to_string(),
            category: Category::Tested,
            ipv6: false,
        };
        let result = fetch(&selection, &client).await.unwrap();
        assert_eq!(result.lines.len(), builtin::SNOWFLAKE.len());
        assert_eq!(result.source, "built-in:snowflake");
    }

    #[tokio::test]
    #[ignore = "requires network access to the collector mirrors"]
    async fn fetch_obfs4_from_collector_live() {
        let client = http_client();
        let selection = Selection {
            transport: "obfs4".to_string(),
            category: Category::FullArchive,
            ipv6: false,
        };
        let result = fetch(&selection, &client).await.unwrap();
        assert!(
            !result.lines.is_empty(),
            "expected obfs4 bridges from collector"
        );
        assert!(result
            .lines
            .iter()
            .any(|l| l.to_ascii_lowercase().starts_with("obfs4")));
        eprintln!(
            "fetched {} obfs4 lines from {}",
            result.lines.len(),
            result.source
        );
    }
}
