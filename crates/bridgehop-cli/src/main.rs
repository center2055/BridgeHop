//! BridgeHop command-line companion.
//!
//! Shares the `bridgehop-core` engine with the desktop app: scan bridge lines (from a file,
//! stdin, or a live source) and fetch bridges from the collector / built-in pools.

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use bridgehop_core::io::{export, qr_svg, to_slipnet_uri, ExportFormat};
use bridgehop_core::sources::{self, Category as SrcCategory, Selection};
use bridgehop_core::store::{RunMeta, Store};
use bridgehop_core::{parse_bridge_lines, scan_bridges, Reachability, ScanOptions, ScanResult};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "bridgehop", version, about = "Tor bridge reachability scanner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan bridge lines for reachability.
    Scan(ScanArgs),
    /// Fetch bridge lines from a source (collector mirror or built-in defaults).
    Sources(SourcesArgs),
    /// Show recorded scan history and per-bridge reliability.
    History(HistoryArgs),
    /// Convert/export bridge lines (plain, torrc, JSON, or a QR code).
    Export(ExportArgs),
}

#[derive(Args)]
struct ExportArgs {
    /// Read bridge lines from a file.
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// Fetch bridge lines from a source transport instead of a file.
    #[arg(long)]
    source: Option<String>,
    /// Source category (used with --source).
    #[arg(long, value_enum, default_value_t = CategoryArg::Tested)]
    category: CategoryArg,
    /// Fetch the IPv6 list (used with --source).
    #[arg(long)]
    ipv6: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = ExportFmt::Plain)]
    format: ExportFmt,
}

#[derive(Copy, Clone, ValueEnum)]
enum ExportFmt {
    Plain,
    Torrc,
    Json,
    /// SVG QR code (encodes the first bridge).
    Qr,
    /// SlipNet `slipnet://` config URIs (one per bridge) for import into SlipNet.
    Slipnet,
}

#[derive(Args)]
struct HistoryArgs {
    /// Show the per-bridge reliability leaderboard instead of the run list.
    #[arg(long)]
    reliability: bool,
    /// Maximum rows to show.
    #[arg(long, default_value_t = 30)]
    limit: usize,
}

#[derive(Args)]
struct ScanArgs {
    /// Read bridge lines from a file.
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// Fetch bridge lines from a source transport (e.g. all, obfs4, snowflake) instead of a file.
    #[arg(long)]
    source: Option<String>,
    /// Source category (used with --source).
    #[arg(long, value_enum, default_value_t = CategoryArg::Tested)]
    category: CategoryArg,
    /// Fetch the IPv6 list (used with --source).
    #[arg(long)]
    ipv6: bool,
    /// Maximum concurrent probes (1-64).
    #[arg(short, long, default_value_t = 16)]
    workers: usize,
    /// Per-probe timeout in milliseconds.
    #[arg(short, long, default_value_t = 3000)]
    timeout: u64,
    /// Deep-verify working bridges by launching the real pluggable-transport client (obfs4).
    #[arg(long)]
    deep: bool,
    /// Output format. `table`/`json` cover every bridge; `plain`/`torrc` emit only the working ones.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,
}

#[derive(Args)]
struct SourcesArgs {
    /// Transport to fetch (all, obfs4, webtunnel, vanilla, snowflake, meek-azure, conjure, dnstt).
    #[arg(default_value = "all")]
    transport: String,
    /// Source category.
    #[arg(long, value_enum, default_value_t = CategoryArg::Tested)]
    category: CategoryArg,
    /// Fetch the IPv6 list (collector transports only).
    #[arg(long)]
    ipv6: bool,
    /// List the available source transports and categories, then exit.
    #[arg(long)]
    list: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = SourcesFormat::Lines)]
    format: SourcesFormat,
}

#[derive(Copy, Clone, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
    /// Only the working bridges, one per line.
    Plain,
    /// Only the working bridges, each as a torrc `Bridge <line>`.
    Torrc,
}

#[derive(Copy, Clone, ValueEnum)]
enum SourcesFormat {
    Lines,
    Json,
}

#[derive(Copy, Clone, ValueEnum)]
enum CategoryArg {
    Tested,
    Fresh72h,
    FullArchive,
}

impl From<CategoryArg> for SrcCategory {
    fn from(value: CategoryArg) -> Self {
        match value {
            CategoryArg::Tested => SrcCategory::Tested,
            CategoryArg::Fresh72h => SrcCategory::Fresh72h,
            CategoryArg::FullArchive => SrcCategory::FullArchive,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let ok = match cli.command {
        Command::Scan(args) => run_scan(args).await,
        Command::Sources(args) => run_sources(args).await,
        Command::History(args) => run_history(args),
        Command::Export(args) => run_export(args).await,
    };
    match ok {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(2)
        }
    }
}

async fn run_scan(args: ScanArgs) -> io::Result<bool> {
    let input = if let Some(path) = &args.file {
        std::fs::read_to_string(path)?
    } else if let Some(transport) = &args.source {
        match load_source(transport, args.category.into(), args.ipv6).await {
            Ok(lines) => lines.join("\n"),
            Err(err) => {
                eprintln!("source error: {err}");
                return Ok(false);
            }
        }
    } else {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    };

    let bridges = parse_bridge_lines(input.lines());
    if bridges.is_empty() {
        eprintln!("no bridge lines found in input");
        return Ok(false);
    }
    let total = bridges.len();

    let options = ScanOptions {
        workers: args.workers,
        timeout: Duration::from_millis(args.timeout),
        deep: args.deep,
    };
    let started_unix = unix_now();
    let source = args.source.clone().unwrap_or_else(|| "manual".to_string());
    let (tx, mut rx) = mpsc::channel(64);
    let cancel = CancellationToken::new();
    let handle = tokio::spawn(async move { scan_bridges(bridges, options, tx, cancel).await });

    let results = match args.format {
        OutputFormat::Table => {
            print_table_header();
            while let Some(result) = rx.recv().await {
                print_row(&result);
            }
            let results = handle.await.expect("scan task panicked");
            print_summary(&results, total);
            results
        }
        OutputFormat::Json => {
            while rx.recv().await.is_some() {}
            let results = handle.await.expect("scan task panicked");
            let json = serde_json::to_string_pretty(&results).expect("results serialize");
            println!("{json}");
            results
        }
        OutputFormat::Plain | OutputFormat::Torrc => {
            while rx.recv().await.is_some() {}
            let results = handle.await.expect("scan task panicked");
            // Emit only the bridges that actually work, so the output is a reusable bridge list.
            let working: Vec<String> = results
                .iter()
                .filter(|r| r.is_working())
                .map(|r| r.raw.clone())
                .collect();
            let fmt = if matches!(args.format, OutputFormat::Torrc) {
                ExportFormat::Torrc
            } else {
                ExportFormat::Plain
            };
            println!("{}", export(&working, fmt));
            // Summary goes to stderr, so redirecting stdout to a file keeps only the bridges.
            print_summary(&results, total);
            results
        }
    };

    // Record the run in the shared database so it shows up in the app's history.
    if !results.is_empty() {
        if let Ok(mut store) = Store::open() {
            let meta = RunMeta {
                started_unix,
                source,
                transport_filter: String::new(),
                deep: args.deep,
            };
            let _ = store.record_run(&meta, &results);
        }
    }

    Ok(results.iter().any(ScanResult::is_working))
}

async fn run_sources(args: SourcesArgs) -> io::Result<bool> {
    if args.list {
        println!("Transports: {}", sources::SOURCE_TRANSPORTS.join(", "));
        println!("Categories: tested, fresh72h, full-archive");
        return Ok(true);
    }

    let client = sources::http_client();
    let selection = Selection {
        transport: args.transport.clone(),
        category: args.category.into(),
        ipv6: args.ipv6,
    };
    match sources::fetch_with_cache(&selection, &client).await {
        Ok(result) => {
            match args.format {
                SourcesFormat::Lines => {
                    for line in &result.lines {
                        println!("{line}");
                    }
                }
                SourcesFormat::Json => {
                    let json = serde_json::to_string_pretty(&result).expect("result serialize");
                    println!("{json}");
                }
            }
            eprintln!("{} bridge(s) from {}", result.lines.len(), result.source);
            Ok(!result.lines.is_empty())
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(false)
        }
    }
}

async fn load_source(
    transport: &str,
    category: SrcCategory,
    ipv6: bool,
) -> Result<Vec<String>, String> {
    let client = sources::http_client();
    let selection = Selection {
        transport: transport.to_string(),
        category,
        ipv6,
    };
    sources::fetch_with_cache(&selection, &client)
        .await
        .map(|result| result.lines)
        .map_err(|err| err.to_string())
}

async fn run_export(args: ExportArgs) -> io::Result<bool> {
    let input = if let Some(path) = &args.file {
        std::fs::read_to_string(path)?
    } else if let Some(transport) = &args.source {
        match load_source(transport, args.category.into(), args.ipv6).await {
            Ok(lines) => lines.join("\n"),
            Err(err) => {
                eprintln!("source error: {err}");
                return Ok(false);
            }
        }
    } else {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    };

    let bridges = parse_bridge_lines(input.lines());
    let lines: Vec<String> = bridges.iter().map(|b| b.raw.clone()).collect();
    if lines.is_empty() {
        eprintln!("no bridges to export");
        return Ok(false);
    }

    match args.format {
        ExportFmt::Plain => println!("{}", export(&lines, ExportFormat::Plain)),
        ExportFmt::Torrc => println!("{}", export(&lines, ExportFormat::Torrc)),
        ExportFmt::Json => println!("{}", export(&lines, ExportFormat::Json)),
        ExportFmt::Qr => match qr_svg(&lines[0]) {
            Ok(svg) => {
                println!("{svg}");
                if lines.len() > 1 {
                    eprintln!("note: QR encodes the first of {} bridges", lines.len());
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(false);
            }
        },
        ExportFmt::Slipnet => {
            for line in &lines {
                if let Some(uri) = to_slipnet_uri(line) {
                    println!("{uri}");
                }
            }
        }
    }
    Ok(true)
}

fn run_history(args: HistoryArgs) -> io::Result<bool> {
    let store = match Store::open() {
        Ok(store) => store,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(false);
        }
    };

    if args.reliability {
        let rows = match store.reliability(args.limit) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(false);
            }
        };
        if rows.is_empty() {
            eprintln!("no reliability data yet — run a scan first");
            return Ok(false);
        }
        println!(
            "{:<11} {:>7} {:>7} {:>9}  BRIDGE",
            "TRANSPORT", "UPTIME", "PROBES", "AVG"
        );
        for r in &rows {
            let avg = r
                .avg_ms
                .map(|m| format!("{m:.0} ms"))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<11} {:>6.0}% {:>7} {:>9}  {}",
                r.transport,
                r.uptime * 100.0,
                r.probes,
                avg,
                truncate(&r.raw, 60)
            );
        }
        return Ok(true);
    }

    let runs = match store.list_runs(args.limit) {
        Ok(runs) => runs,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(false);
        }
    };
    if runs.is_empty() {
        eprintln!("no scan runs recorded yet");
        return Ok(false);
    }
    println!(
        "{:>5} {:<12} {:>6} {:>8} {:>6}  SOURCE",
        "RUN", "WHEN", "TOTAL", "WORKING", "DOWN"
    );
    for r in &runs {
        let working = r.reachable + r.slow + r.fronted;
        println!(
            "{:>5} {:<12} {:>6} {:>8} {:>6}  {}",
            r.id,
            ago(r.started_unix),
            r.total,
            working,
            r.unreachable,
            truncate(&r.source, 50)
        );
    }
    Ok(true)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text.to_string()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ago(unix: u64) -> String {
    let secs = unix_now().saturating_sub(unix);
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

fn print_table_header() {
    println!(
        "{:<7} {:>8} {:<11} {:<30} DETAIL",
        "STATUS", "PING", "TRANSPORT", "HOST:PORT"
    );
}

fn print_row(result: &ScanResult) {
    let host_port = format!("{}:{}", result.probed_host, result.probed_port);
    let ping = result
        .ping_ms
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| "-".to_string());
    let detail = match &result.deep {
        Some(d) if d.ok => format!(
            "{} · deep OK ({} ms)",
            result.detail,
            d.socks_ms.unwrap_or(0)
        ),
        Some(d) => format!("{} · deep FAIL: {}", result.detail, d.detail),
        None => result.detail.clone(),
    };
    println!(
        "{:<7} {:>8} {:<11} {:<30} {}",
        status_label(result.reachability),
        ping,
        result.transport.token(),
        host_port,
        detail
    );
}

fn status_label(reachability: Reachability) -> &'static str {
    match reachability {
        Reachability::Reachable => "OK",
        Reachability::Slow => "SLOW",
        Reachability::Unreachable => "DOWN",
        Reachability::Unparsed => "SKIP",
        Reachability::Fronted => "FRONT",
    }
}

fn print_summary(results: &[ScanResult], total: usize) {
    let mut reachable = 0;
    let mut slow = 0;
    let mut fronted = 0;
    let mut unreachable = 0;
    let mut unparsed = 0;
    for result in results {
        match result.reachability {
            Reachability::Reachable => reachable += 1,
            Reachability::Slow => slow += 1,
            Reachability::Fronted => fronted += 1,
            Reachability::Unreachable => unreachable += 1,
            Reachability::Unparsed => unparsed += 1,
        }
    }
    let working = reachable + slow + fronted;
    eprintln!(
        "\n{total} scanned — {working} working ({reachable} reachable, {slow} slow, \
         {fronted} fronted), {unreachable} unreachable, {unparsed} skipped"
    );
}
