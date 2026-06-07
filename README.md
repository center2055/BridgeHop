# BridgeHop

A lightweight, cross-platform **Tor bridge reachability scanner** with a modern UI.

BridgeHop tests whether Tor bridges are reachable from your network across **all bridge
types** — `vanilla`, `obfs4`, `webtunnel`, `snowflake`, `meek`, `conjure`, and `dnstt` — so
you can quickly find bridges that actually work where you are. It ships with built-in bridge
lists and live sources, and you can add your own.

## Features

- **Layered reachability checks**
  - Fast TCP-connect probe for direct transports (`vanilla`, `obfs4`, …).
  - Front-host TLS probe on `:443` for fronted/broker transports (`snowflake`, `meek`,
    `conjure`, `dnstt`, `webtunnel`).
  - Optional **deep verify** (desktop) that launches the real pluggable-transport client to
    confirm an actual handshake. Transport binaries are fetched on demand to keep the base
    app tiny.
- **Built-in lists & live sources** — community collector mirrors, Tor Moat, BridgeDB,
  bundled seed lists, plus your own manually added bridges.
- **Scan history & reliability** — track which bridges keep working over time.
- **Geo / ASN / latency insights** — see where a bridge lives and how fast it responds.
- **Import / export** — plain lines, `torrc`, JSON, and QR codes.
- **CLI companion** — scriptable scanning that shares the same engine as the app.

## Architecture

BridgeHop is a [Tauri](https://tauri.app) application: a Rust core does all the work, a small
web UI renders it, and a CLI reuses the same core.

```
crates/bridgehop-core   # parsing, scanning engine, sources, storage, geo, import/export
crates/bridgehop-cli    # command-line companion
src-tauri               # Tauri desktop shell (thin command/event layer)
ui                      # SvelteKit + Tailwind front end
```

## Building from source

Requirements: a recent stable [Rust](https://rustup.rs) toolchain and
[Node.js](https://nodejs.org) 20+.

```sh
# Front end deps
cd ui && npm install && cd ..

# Run the desktop app in dev mode
npm --prefix ui run tauri dev

# Build the CLI
cargo build --release -p bridgehop-cli

# Run the test suite
cargo test --workspace
```

## License

BridgeHop is free software, licensed under the
[GNU General Public License v3.0 or later](LICENSE).
