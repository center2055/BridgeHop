//! BridgeHop command-line companion.
//!
//! Shares the `bridgehop-core` engine with the desktop app: scan bridge lines (from a file,
//! stdin, or a live source) and fetch bridges from the collector / built-in pools.

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use bridgehop_core::sources::{self, Category as SrcCategory, Selection};
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
    /// Output format.
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
        deep: false,
    };
    let (tx, mut rx) = mpsc::channel(64);
    let cancel = CancellationToken::new();
    let handle = tokio::spawn(async move { scan_bridges(bridges, options, tx, cancel).await });

    let any_working = match args.format {
        OutputFormat::Table => {
            print_table_header();
            while let Some(result) = rx.recv().await {
                print_row(&result);
            }
            let results = handle.await.expect("scan task panicked");
            print_summary(&results, total);
            results.iter().any(ScanResult::is_working)
        }
        OutputFormat::Json => {
            while rx.recv().await.is_some() {}
            let results = handle.await.expect("scan task panicked");
            let json = serde_json::to_string_pretty(&results).expect("results serialize");
            println!("{json}");
            results.iter().any(ScanResult::is_working)
        }
    };

    Ok(any_working)
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
    match sources::fetch(&selection, &client).await {
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
    sources::fetch(&selection, &client)
        .await
        .map(|result| result.lines)
        .map_err(|err| err.to_string())
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
    println!(
        "{:<7} {:>8} {:<11} {:<30} {}",
        status_label(result.reachability),
        ping,
        result.transport.token(),
        host_port,
        result.detail
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
