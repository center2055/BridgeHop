//! SQLite persistence: scan runs, per-result history, and per-bridge reliability.
//!
//! Uses a bundled SQLite (statically linked, no system dependency). The database lives at
//! `<data-dir>/bridgehop.db`. Reliability is computed on the fly with a `GROUP BY` query rather
//! than maintained as a table.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::ScanResult;
use crate::parse::parse_bridge_line;
use crate::paths;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS bridge (
    id           TEXT PRIMARY KEY,
    raw          TEXT NOT NULL,
    transport    TEXT NOT NULL,
    host         TEXT,
    port         INTEGER,
    fingerprint  TEXT,
    front_host   TEXT,
    params_json  TEXT,
    country      TEXT,
    asn          INTEGER,
    as_org       TEXT,
    first_seen   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL,
    source       TEXT,
    label        TEXT
);

CREATE TABLE IF NOT EXISTS scan_run (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    started_unix     INTEGER NOT NULL,
    finished_unix    INTEGER NOT NULL,
    source           TEXT,
    transport_filter TEXT,
    deep             INTEGER NOT NULL DEFAULT 0,
    total            INTEGER NOT NULL,
    reachable        INTEGER NOT NULL,
    slow             INTEGER NOT NULL,
    fronted          INTEGER NOT NULL,
    unreachable      INTEGER NOT NULL,
    unparsed         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS scan_result (
    run_id       INTEGER NOT NULL REFERENCES scan_run(id) ON DELETE CASCADE,
    bridge_id    TEXT NOT NULL REFERENCES bridge(id),
    reachability TEXT NOT NULL,
    ping_ms      INTEGER,
    detail       TEXT,
    deep_ok      INTEGER,
    deep_ms      INTEGER,
    probed_host  TEXT,
    probed_port  INTEGER,
    ts_unix      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_result_bridge ON scan_result(bridge_id, ts_unix);
CREATE INDEX IF NOT EXISTS ix_result_run ON scan_result(run_id);
"#;

/// Metadata describing a scan run being recorded.
#[derive(Debug, Clone)]
pub struct RunMeta {
    pub started_unix: u64,
    pub source: String,
    pub transport_filter: String,
    pub deep: bool,
}

/// A summary row for a past scan run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub id: i64,
    pub started_unix: u64,
    pub finished_unix: u64,
    pub source: String,
    pub transport_filter: String,
    pub deep: bool,
    pub total: u32,
    pub reachable: u32,
    pub slow: u32,
    pub fronted: u32,
    pub unreachable: u32,
    pub unparsed: u32,
}

/// Aggregated reliability for a single bridge across all recorded scans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reliability {
    pub bridge_id: String,
    pub raw: String,
    pub transport: String,
    pub country: Option<String>,
    pub asn: Option<u32>,
    /// Fraction of probes that were working (0.0..=1.0).
    pub uptime: f64,
    pub probes: u32,
    pub avg_ms: Option<f64>,
    pub last_unix: u64,
}

/// The SQLite-backed store.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (and migrate) the database at the default data-dir location.
    pub fn open() -> Result<Store> {
        let dir = paths::ensure_data_dir()?;
        Self::open_at(dir.join("bridgehop.db"))
    }

    /// Open (and migrate) the database at a specific path.
    pub fn open_at(path: impl AsRef<std::path::Path>) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    /// Open an in-memory database (used in tests).
    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    /// Delete all recorded scans (runs, results, and bridges), resetting history and reliability.
    pub fn clear(&mut self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM scan_result; DELETE FROM scan_run; DELETE FROM bridge;")?;
        Ok(())
    }

    /// Persist a completed scan run and all of its results. Returns the new run id.
    pub fn record_run(&mut self, meta: &RunMeta, results: &[ScanResult]) -> Result<i64> {
        let now = now_unix();
        let mut reachable = 0u32;
        let mut slow = 0u32;
        let mut fronted = 0u32;
        let mut unreachable = 0u32;
        let mut unparsed = 0u32;
        for r in results {
            use crate::model::Reachability::*;
            match r.reachability {
                Reachable => reachable += 1,
                Slow => slow += 1,
                Fronted => fronted += 1,
                Unreachable => unreachable += 1,
                Unparsed => unparsed += 1,
            }
        }

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO scan_run \
             (started_unix, finished_unix, source, transport_filter, deep, total, reachable, slow, fronted, unreachable, unparsed) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                meta.started_unix,
                now,
                meta.source,
                meta.transport_filter,
                meta.deep as i64,
                results.len() as i64,
                reachable,
                slow,
                fronted,
                unreachable,
                unparsed
            ],
        )?;
        let run_id = tx.last_insert_rowid();

        {
            let mut upsert_bridge = tx.prepare(
                "INSERT INTO bridge (id, raw, transport, host, port, fingerprint, front_host, params_json, country, asn, as_org, first_seen, last_seen, source) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13) \
                 ON CONFLICT(id) DO UPDATE SET last_seen = excluded.last_seen, raw = excluded.raw, \
                 transport = excluded.transport, host = excluded.host, port = excluded.port, \
                 fingerprint = excluded.fingerprint, front_host = excluded.front_host, \
                 params_json = excluded.params_json, \
                 country = COALESCE(excluded.country, country), \
                 asn = COALESCE(excluded.asn, asn), \
                 as_org = COALESCE(excluded.as_org, as_org)",
            )?;
            let mut insert_result = tx.prepare(
                "INSERT INTO scan_result \
                 (run_id, bridge_id, reachability, ping_ms, detail, deep_ok, deep_ms, probed_host, probed_port, ts_unix) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;

            for r in results {
                let parsed = parse_bridge_line(&r.raw);
                let fingerprint = parsed.as_ref().and_then(|b| b.fingerprint.clone());
                let front_host = parsed.as_ref().and_then(|b| b.front_host.clone());
                let params_json = parsed
                    .as_ref()
                    .and_then(|b| serde_json::to_string(&b.params).ok());
                let (country, asn, as_org) = match &r.geo {
                    Some(g) => (g.country.clone(), g.asn.map(|a| a as i64), g.as_org.clone()),
                    None => (None, None, None),
                };

                upsert_bridge.execute(params![
                    r.bridge_id,
                    r.raw,
                    r.transport.token(),
                    r.probed_host,
                    r.probed_port,
                    fingerprint,
                    front_host,
                    params_json,
                    country,
                    asn,
                    as_org,
                    now,
                    meta.source,
                ])?;

                let deep_ok = r.deep.as_ref().map(|d| d.ok as i64);
                let deep_ms = r.deep.as_ref().and_then(|d| d.socks_ms.map(|m| m as i64));

                insert_result.execute(params![
                    run_id,
                    r.bridge_id,
                    r.reachability.as_str(),
                    r.ping_ms.map(|m| m as i64),
                    r.detail,
                    deep_ok,
                    deep_ms,
                    r.probed_host,
                    r.probed_port,
                    now,
                ])?;
            }
        }

        tx.commit()?;
        Ok(run_id)
    }

    /// Most recent scan runs, newest first.
    pub fn list_runs(&self, limit: usize) -> Result<Vec<RunSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_unix, finished_unix, source, transport_filter, deep, \
             total, reachable, slow, fronted, unreachable, unparsed \
             FROM scan_run ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(RunSummary {
                id: row.get(0)?,
                started_unix: row.get::<_, i64>(1)? as u64,
                finished_unix: row.get::<_, i64>(2)? as u64,
                source: row.get(3)?,
                transport_filter: row.get(4)?,
                deep: row.get::<_, i64>(5)? != 0,
                total: row.get::<_, i64>(6)? as u32,
                reachable: row.get::<_, i64>(7)? as u32,
                slow: row.get::<_, i64>(8)? as u32,
                fronted: row.get::<_, i64>(9)? as u32,
                unreachable: row.get::<_, i64>(10)? as u32,
                unparsed: row.get::<_, i64>(11)? as u32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Per-bridge reliability leaderboard, best uptime (then lowest latency) first.
    pub fn reliability(&self, limit: usize) -> Result<Vec<Reliability>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.raw, b.transport, b.country, b.asn, \
             AVG(CASE WHEN r.reachability IN ('reachable','slow','fronted') THEN 1.0 ELSE 0.0 END) AS uptime, \
             COUNT(*) AS probes, AVG(r.ping_ms) AS avg_ms, MAX(r.ts_unix) AS last_unix \
             FROM scan_result r JOIN bridge b ON b.id = r.bridge_id \
             GROUP BY r.bridge_id \
             ORDER BY uptime DESC, avg_ms ASC \
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(Reliability {
                bridge_id: row.get(0)?,
                raw: row.get(1)?,
                transport: row.get(2)?,
                country: row.get(3)?,
                asn: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
                uptime: row.get(5)?,
                probes: row.get::<_, i64>(6)? as u32,
                avg_ms: row.get(7)?,
                last_unix: row.get::<_, i64>(8)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Reachability, ScanResult, Transport};

    fn sample(reachability: Reachability, ping: Option<u32>) -> ScanResult {
        ScanResult {
            bridge_id: "abc123".to_string(),
            raw: "1.2.3.4:8080 A7E7616C91B2FD83005B986A816EE9365F1360F4".to_string(),
            transport: Transport::Vanilla,
            probed_host: "1.2.3.4".to_string(),
            probed_port: 8080,
            ping_ms: ping,
            reachability,
            detail: "test".to_string(),
            deep: None,
            geo: None,
        }
    }

    #[test]
    fn records_run_and_lists_it() {
        let mut store = Store::open_in_memory().unwrap();
        let meta = RunMeta {
            started_unix: 100,
            source: "manual".to_string(),
            transport_filter: "all".to_string(),
            deep: false,
        };
        let id = store
            .record_run(&meta, &[sample(Reachability::Reachable, Some(120))])
            .unwrap();
        assert!(id > 0);

        let runs = store.list_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].total, 1);
        assert_eq!(runs[0].reachable, 1);
        assert_eq!(runs[0].source, "manual");
    }

    #[test]
    fn clear_wipes_runs_and_reliability() {
        let mut store = Store::open_in_memory().unwrap();
        let meta = RunMeta {
            started_unix: 100,
            source: "manual".to_string(),
            transport_filter: "all".to_string(),
            deep: false,
        };
        store
            .record_run(&meta, &[sample(Reachability::Reachable, Some(120))])
            .unwrap();
        assert_eq!(store.list_runs(10).unwrap().len(), 1);

        store.clear().unwrap();
        assert!(store.list_runs(10).unwrap().is_empty());
        assert!(store.reliability(10).unwrap().is_empty());
    }

    #[test]
    fn reliability_aggregates_across_runs() {
        let mut store = Store::open_in_memory().unwrap();
        let meta = RunMeta {
            started_unix: 1,
            source: "manual".to_string(),
            transport_filter: "all".to_string(),
            deep: false,
        };
        // Same bridge: one working, one down -> 50% uptime over 2 probes.
        store
            .record_run(&meta, &[sample(Reachability::Reachable, Some(100))])
            .unwrap();
        store
            .record_run(&meta, &[sample(Reachability::Unreachable, None)])
            .unwrap();

        let rel = store.reliability(10).unwrap();
        assert_eq!(rel.len(), 1);
        assert_eq!(rel[0].probes, 2);
        assert!((rel[0].uptime - 0.5).abs() < 1e-9);
        assert_eq!(rel[0].avg_ms, Some(100.0));
    }
}
