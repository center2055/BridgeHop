//! BridgeHop command-line companion.
//!
//! Shares the `bridgehop-core` engine with the desktop app. This first version exposes a `scan`
//! subcommand; sources/import/export/history land in a later phase.

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

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
}

#[derive(Args)]
struct ScanArgs {
    /// Read bridge lines from a file (defaults to standard input).
    #[arg(short, long)]
    file: Option<PathBuf>,
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

#[derive(Copy, Clone, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan(args) => match run_scan(args).await {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE, // no working bridges -> scriptable failure
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::from(2)
            }
        },
    }
}

async fn run_scan(args: ScanArgs) -> io::Result<bool> {
    let input = match &args.file {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
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
            while rx.recv().await.is_some() {} // drain streamed results
            let results = handle.await.expect("scan task panicked");
            let json = serde_json::to_string_pretty(&results).expect("results serialize");
            println!("{json}");
            results.iter().any(ScanResult::is_working)
        }
    };

    Ok(any_working)
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
