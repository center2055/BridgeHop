# EVIDENCE

Rule R3: only what was run in this session, with its exit code, is listed as verified. Everything
else is UNVERIFIED. Machine: Linux x86_64 sandbox, 2 cores, 3 GB RAM, 2026-11-27.
Raw logs: `evidence/` in the delivered bundle (not all are in the repository).

## Verified this session

| What | Command | Result | Exit |
| --- | --- | --- | --- |
| Rust tests (default members) | `cargo test` | bridgehop-core 43 passed / 1 ignored; hf-core 13 + 18 + 7 passed | 0 |
| Rust lints, after the last engine change | `cargo clippy --all-targets -- -D warnings` | no warning | 0 |
| Host pluggable transports | `pt-sidecars/build.sh` (linux/amd64) | 5 binaries, modules checked with `go version -m` | 0 |
| Android arm64 pluggable transports | `pt-sidecars/build.sh` (android/arm64-v8a) | 5 binaries, LOAD alignment 0x10000 (16 KB) | 0 |
| Android shell compiles | kotlinc 2.4.21, then aapt2 + d8 precheck | 0 diagnostics, classes.dex 25520 bytes | 0 |
| tor for Android | download of `tor-android-0.4.9.17.1.aar` | sha256 matched the pinned value | 0 |
| **Real end-to-end run on the host** | `scripts/e2e-host.sh` with the 85 bundled bridge lines | see below | 0 |

### The end-to-end run (host build of the same engine the APK uses)

```
bridges sent: 85
engine reports connected; SOCKS port 46791. Independent check with curl:
  curl exit 0 : {"IsTor":true,"IP":"192.42.116.55"}
  www.torproject.org via SOCKS: HTTP 200 (curl exit 0)
events: 12 probe, 1 plan, 26 progress, 20 log, 3 tier (T3, T4, T5 all pass), 1 connected
```

- T1: 7 of the 12 obfs4 bridges probed in the first wave answered (175–232 ms); 5 timed out.
- T3 (transport handshake) and T4 (bootstrap 0 % → 100 %) were observed through tor's control
  port; bootstrap took about 8 seconds.
- T5: the engine's own check and a second, independent client (curl) both went through the
  engine's SOCKS port and were confirmed as Tor exits.
- Tier T4+ on a desktop, **not on a phone**: the Android build is not in this table.

Bugs the run exposed in the engine, all fixed in `crates/hf-engine/src/main.rs`:

1. The transport path in `ClientTransportPlugin … exec` was quoted. tor splits that line on
   spaces and knows no quoting (checked in tor 0.4.8.25's `config.c`), so the quotes became part
   of the file name, no transport started and bootstrap stayed at 0 %.
2. tor keeps only the last of several bridge lines that share an address and port
   (`bridge_resolve_conflicts` in `bridges.c`), and every fronted transport uses the same
   placeholder endpoint: only one line per fronted transport is written now, and the snowflake
   line wins over the conjure one.
3. Control-port status events are `650 STATUS_CLIENT NOTICE BOOTSTRAP PROGRESS=…`; the engine
   parsed them in the other order, so it never saw progress and reported "timed out at 0 %" while
   tor had already bootstrapped.

## Not verified (UNVERIFIED)

- **APK**: `apps/android/build.sh` did not complete. Two build failures were fixed (rustflag
  encoding, NDK r30 target triple) but the build was not rerun afterwards. There is no APK.
- **On a phone, in Termux, in Iran**: nothing was run on a device. `scripts/termux-build.sh` in
  the tree is the first draft: it still installs a `zipalign` package that does not exist in the
  Termux index (verified on 2026-11-27) and must not be used as it is.
- **Google Play Protect**: a server-side verdict cannot be produced here. Only the hygiene rules
  (minimal permissions, hardened WebView, v2+v3 signature, 16 KB alignment, `verify.sh`) exist.
- **Supply chain**: `cargo deny` exits 7 (6 rejected licenses, 9 RustSec advisories, wildcard
  dependencies). `cargo-audit` 0.23.0 is installed but was not run.
- **Desktop app** (`src-tauri`): needs `gdk-3.0`, not available here; not built, not tested.
- `cargo test` was last run before the final engine changes; those changes are in a binary crate
  without tests of its own and are covered by the end-to-end run above.
