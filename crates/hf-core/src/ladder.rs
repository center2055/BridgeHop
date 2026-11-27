//! The verification ladder: what "working" means and how an observation becomes a status.
//!
//! Every result HopForge keeps is an [`Observation`] made by a probe that ran, at a [`Tier`],
//! from a [`Vantage`], at a moment in time. Nothing here infers: an [`Outcome::Inconclusive`]
//! stays inconclusive, an observation is never promoted to a tier that was not actually run, and
//! a bridge is [`Status::Working`] only while it holds a fresh pass at [`Tier::T4`] or above.
//! Observations from different vantages are kept apart by the caller (one [`Evidence`] per
//! vantage), because a pass seen from a data-centre in Europe says nothing about an ISP in Iran.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::transport::Transport;

/// Lowest tier whose pass lets the UI say a bridge works. T0–T3 only rank candidates.
pub const MIN_WORKING_TIER: Tier = Tier::T4;

/// A pass older than this no longer counts as working. Bridge collectors publish on a 72-hour
/// cycle, and a bridge that worked three days ago is routinely gone today.
pub const MAX_AGE_SECS: u64 = 72 * 60 * 60;

/// One rung of the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// The line parses and every argument the transport needs is present.
    T0,
    /// A TCP connection to the bridge's own address is established.
    T1,
    /// A TLS handshake with the address completes.
    T2,
    /// The pluggable transport completes its own handshake with the bridge.
    T3,
    /// Tor reaches 100% bootstrap through the bridge.
    T4,
    /// A real request is fetched end to end through the Tor circuit.
    T5,
    /// T5 keeps succeeding over a long run with an acceptable failure rate.
    T6,
}

impl Tier {
    /// Every tier, bottom first.
    pub const ALL: [Tier; 7] = [
        Tier::T0,
        Tier::T1,
        Tier::T2,
        Tier::T3,
        Tier::T4,
        Tier::T5,
        Tier::T6,
    ];

    /// Position in the ladder, `0` for T0.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Short human name, used in badges and exported reports.
    pub const fn label(self) -> &'static str {
        match self {
            Tier::T0 => "valid line",
            Tier::T1 => "TCP connect",
            Tier::T2 => "TLS handshake",
            Tier::T3 => "transport handshake",
            Tier::T4 => "Tor bootstrap",
            Tier::T5 => "end to end",
            Tier::T6 => "stable",
        }
    }

    /// The next rung, if any.
    pub const fn next(self) -> Option<Tier> {
        match self {
            Tier::T0 => Some(Tier::T1),
            Tier::T1 => Some(Tier::T2),
            Tier::T2 => Some(Tier::T3),
            Tier::T3 => Some(Tier::T4),
            Tier::T4 => Some(Tier::T5),
            Tier::T5 => Some(Tier::T6),
            Tier::T6 => None,
        }
    }

    /// Whether this rung can be run for a transport at all.
    ///
    /// Fronted transports carry a placeholder address, so connecting to it proves nothing and is
    /// never attempted. obfs4 has no TLS layer, and vanilla has no pluggable transport, so each
    /// skips the rung that does not exist for it.
    pub fn applies(self, transport: &Transport) -> bool {
        match self {
            Tier::T0 | Tier::T4 | Tier::T5 | Tier::T6 => true,
            Tier::T1 => transport.is_direct(),
            Tier::T2 => matches!(transport, Transport::Vanilla | Transport::Webtunnel),
            // The transport handshake needs a pluggable transport client; vanilla has none (its
            // link handshake is part of bootstrap, T4).
            Tier::T3 => transport.sidecar().is_some(),
        }
    }

    /// The first rung that can actually be run for this transport, given that T0 passed.
    pub fn first_probe(transport: &Transport) -> Option<Tier> {
        Tier::ALL
            .into_iter()
            .skip(1)
            .find(|tier| tier.applies(transport))
    }
}

/// What a probe saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The rung's success condition was met.
    Pass,
    /// The rung's failure condition was met: the peer answered and was refused, or a definitive
    /// protocol error occurred.
    Fail,
    /// The probe could not decide: timeout, local network down, probe budget exhausted. Never
    /// counted as a pass or a fail.
    Inconclusive,
    /// The rung was not run (does not apply, budget spent, user stopped).
    Skipped,
}

impl Outcome {
    /// Whether this outcome says the rung succeeded or failed, as opposed to saying nothing.
    pub const fn is_decisive(self) -> bool {
        matches!(self, Outcome::Pass | Outcome::Fail)
    }
}

/// Where an observation was made from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vantage {
    /// Free-form name shown to the user, e.g. "home network, Iran".
    pub label: String,
    /// ISO 3166-1 alpha-2 country code, when known.
    pub country: Option<String>,
    /// Whether the vantage is inside the country the build targets. Set from a field report,
    /// never from an IP-geolocation lookup.
    pub in_target_country: bool,
}

impl Vantage {
    /// The machine that ran a given check, which is outside Iran. Every automated result
    /// produced by HopForge's own CI and build scripts carries this vantage.
    pub fn runner_side() -> Self {
        Self {
            label: "runner-side, outside Iran".to_string(),
            country: None,
            in_target_country: false,
        }
    }
}

/// One probe result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// The rung that was run.
    pub tier: Tier,
    /// What it saw.
    pub outcome: Outcome,
    /// Where it ran.
    pub vantage: Vantage,
    /// Unix time, seconds, when the result was produced.
    pub observed_at: u64,
    /// Round-trip time of the rung's main network exchange, when one was measured.
    pub rtt_ms: Option<u32>,
    /// Short machine-and-human readable note (log line, error class). No bridge secrets.
    pub detail: String,
}

impl Observation {
    /// A pass at `tier`.
    pub fn pass(
        tier: Tier,
        vantage: Vantage,
        observed_at: u64,
        rtt_ms: Option<u32>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            tier,
            outcome: Outcome::Pass,
            vantage,
            observed_at,
            rtt_ms,
            detail: detail.into(),
        }
    }

    /// A non-pass result at `tier`.
    pub fn new(
        tier: Tier,
        outcome: Outcome,
        vantage: Vantage,
        observed_at: u64,
        rtt_ms: Option<u32>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            tier,
            outcome,
            vantage,
            observed_at,
            rtt_ms,
            detail: detail.into(),
        }
    }
}

/// How far a bridge has been verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    /// No rung has been passed.
    Unverified,
    /// T0.
    StaticOnly,
    /// T1.
    Reachable,
    /// T3.
    TransportHandshake,
    /// T4.
    Bootstrapped,
    /// T5.
    EndToEnd,
    /// T6.
    Stable,
}

impl EvidenceLevel {
    /// The level a pass at `tier` grants. T2 grants nothing beyond T1: a completed TLS
    /// handshake is a property of the same TCP connection.
    pub const fn for_pass(tier: Tier) -> Self {
        match tier {
            Tier::T0 => EvidenceLevel::StaticOnly,
            Tier::T1 | Tier::T2 => EvidenceLevel::Reachable,
            Tier::T3 => EvidenceLevel::TransportHandshake,
            Tier::T4 => EvidenceLevel::Bootstrapped,
            Tier::T5 => EvidenceLevel::EndToEnd,
            Tier::T6 => EvidenceLevel::Stable,
        }
    }

    /// Whether this level may be shown as working.
    pub const fn is_working(self) -> bool {
        matches!(
            self,
            EvidenceLevel::Bootstrapped | EvidenceLevel::EndToEnd | EvidenceLevel::Stable
        )
    }
}

/// What the UI says about a bridge, from one vantage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// A fresh pass at T4 or above.
    Working,
    /// A pass at T4 or above, but older than [`MAX_AGE_SECS`].
    Stale,
    /// The most recent pass-or-fail observation is a fail.
    Failed,
    /// Nothing decisive; at least one rung was inconclusive.
    Inconclusive,
    /// No rung beyond the reachability checks has a decisive result.
    Unverified,
}

/// What is known about one bridge from one vantage.
///
/// Two records are kept per rung: the most recent *decisive* observation (a pass or a fail) and
/// the most recent attempt of any kind. An inconclusive or skipped result therefore never
/// displaces a real result; it only records that the rung was tried.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// Most recent pass or fail per rung.
    decisive: BTreeMap<Tier, Observation>,
    /// Most recent attempt per rung, whatever it saw.
    last_attempt: BTreeMap<Tier, Observation>,
}

impl Evidence {
    /// Empty evidence.
    pub const fn new() -> Self {
        Self {
            decisive: BTreeMap::new(),
            last_attempt: BTreeMap::new(),
        }
    }

    /// Records an observation. Results can arrive out of order, so an observation replaces the
    /// stored one for its rung only when it is at least as recent.
    pub fn record(&mut self, obs: Observation) {
        let newer = |old: &Observation| obs.observed_at >= old.observed_at;
        if self.last_attempt.get(&obs.tier).is_none_or(newer) {
            self.last_attempt.insert(obs.tier, obs.clone());
        }
        if obs.outcome.is_decisive() && self.decisive.get(&obs.tier).is_none_or(newer) {
            self.decisive.insert(obs.tier, obs);
        }
    }

    /// The most recent pass or fail for a rung.
    pub fn decisive(&self, tier: Tier) -> Option<&Observation> {
        self.decisive.get(&tier)
    }

    /// The most recent attempt for a rung, whatever it saw.
    pub fn last_attempt(&self, tier: Tier) -> Option<&Observation> {
        self.last_attempt.get(&tier)
    }

    /// The most recent attempt of any kind.
    pub fn most_recent(&self) -> Option<&Observation> {
        self.last_attempt.values().max_by_key(|obs| obs.observed_at)
    }

    /// The decisive observations, bottom rung first.
    pub fn all(&self) -> impl Iterator<Item = &Observation> {
        self.decisive.values()
    }

    /// The highest applicable rung that has a pass.
    pub fn level(&self, transport: &Transport) -> EvidenceLevel {
        Tier::ALL
            .into_iter()
            .filter(|tier| tier.applies(transport))
            .filter(|tier| {
                self.decisive(*tier)
                    .is_some_and(|obs| obs.outcome == Outcome::Pass)
            })
            .map(EvidenceLevel::for_pass)
            .max()
            .unwrap_or(EvidenceLevel::Unverified)
    }

    /// The status at time `now` (Unix seconds).
    ///
    /// The decisive observation is the most recent pass or fail on an applicable rung (ties go
    /// to the higher rung). Inconclusive and skipped results never decide anything.
    pub fn status(&self, transport: &Transport, now: u64) -> Status {
        let decisive = Tier::ALL
            .into_iter()
            .filter(|tier| tier.applies(transport))
            .filter_map(|tier| self.decisive(tier))
            .max_by_key(|obs| (obs.observed_at, obs.tier));
        match decisive {
            Some(obs) if obs.outcome == Outcome::Fail => Status::Failed,
            Some(obs) if obs.tier < MIN_WORKING_TIER => Status::Unverified,
            Some(obs) if now.saturating_sub(obs.observed_at) > MAX_AGE_SECS => Status::Stale,
            Some(_) => Status::Working,
            None if self.tried_inconclusively(transport) => Status::Inconclusive,
            None => Status::Unverified,
        }
    }

    fn tried_inconclusively(&self, transport: &Transport) -> bool {
        self.last_attempt
            .iter()
            .any(|(tier, obs)| tier.applies(transport) && obs.outcome == Outcome::Inconclusive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obfs4() -> Transport {
        Transport::Obfs4
    }

    #[test]
    fn tier_order_and_labels() {
        assert!(Tier::T0 < Tier::T6);
        assert_eq!(Tier::T0.index(), 0);
        assert_eq!(Tier::T6.index(), 6);
        assert_eq!(Tier::T3.next(), Some(Tier::T4));
        assert_eq!(Tier::T6.next(), None);
        assert_eq!(Tier::ALL.len(), 7);
        for pair in Tier::ALL.windows(2) {
            assert!(pair[0] < pair[1]);
        }
        assert_eq!(serde_json::to_string(&Tier::T4).unwrap(), "\"t4\"");
        assert_eq!(serde_json::from_str::<Tier>("\"t2\"").unwrap(), Tier::T2);
    }

    #[test]
    fn applicability_follows_the_transport_shape() {
        // obfs4: its own address, no TLS layer.
        assert_eq!(Tier::first_probe(&Transport::Obfs4), Some(Tier::T1));
        assert!(Tier::T1.applies(&Transport::Obfs4));
        assert!(!Tier::T2.applies(&Transport::Obfs4));
        assert!(Tier::T3.applies(&Transport::Obfs4));
        // webtunnel: its own address, TLS inside.
        assert!(Tier::T2.applies(&Transport::Webtunnel));
        // vanilla: no pluggable transport to handshake with.
        assert!(Tier::T1.applies(&Transport::Vanilla));
        assert!(Tier::T2.applies(&Transport::Vanilla));
        assert!(!Tier::T3.applies(&Transport::Vanilla));
        // fronted: the address is a placeholder.
        for fronted in [Transport::Snowflake, Transport::Conjure, Transport::Dnstt] {
            assert!(!Tier::T1.applies(&fronted));
            assert!(!Tier::T2.applies(&fronted));
            assert!(Tier::T3.applies(&fronted));
            assert_eq!(Tier::first_probe(&fronted), Some(Tier::T3));
        }
        // A fronted transport with a sidecar starts at the transport handshake.
        assert_eq!(Tier::first_probe(&Transport::MeekLite), Some(Tier::T3));
        assert!(Tier::T4.applies(&Transport::MeekLite));
        // A transport without a sidecar can only be TCP-probed at its line address; its
        // handshake and the Tor rungs need the transport's own client.
        let unknown = Transport::Unknown("obfs3".into());
        assert_eq!(Tier::first_probe(&unknown), Some(Tier::T1));
        assert!(!Tier::T3.applies(&unknown));
        assert!(Tier::T4.applies(&unknown) && Tier::T6.applies(&unknown));
    }

    #[test]
    fn level_counts_only_applicable_passes() {
        let v = Vantage::runner_side();
        let mut evidence = Evidence::new();
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::Unverified);
        evidence.record(Observation::pass(Tier::T0, v.clone(), 100, None, "parsed"));
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::StaticOnly);
        evidence.record(Observation::pass(
            Tier::T1,
            v.clone(),
            101,
            Some(40),
            "connected",
        ));
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::Reachable);
        // A fronted transport cannot hold a T1 pass: it is ignored for its level.
        assert_eq!(
            evidence.level(&Transport::Snowflake),
            EvidenceLevel::StaticOnly
        );
        evidence.record(Observation::pass(
            Tier::T3,
            v.clone(),
            102,
            Some(900),
            "obfs4 handshake",
        ));
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::TransportHandshake);
        evidence.record(Observation::pass(
            Tier::T4,
            v.clone(),
            103,
            Some(9000),
            "bootstrapped",
        ));
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::Bootstrapped);
        assert!(evidence.level(&obfs4()).is_working());
        evidence.record(Observation::pass(
            Tier::T5,
            v.clone(),
            104,
            Some(2500),
            "200 from check.torproject.org",
        ));
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::EndToEnd);
        evidence.record(Observation::pass(Tier::T6, v, 105, Some(2500), "stable"));
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::Stable);
        // A failure does not lower the level of the pass already recorded at that tier.
        assert!(!EvidenceLevel::TransportHandshake.is_working());
        assert!(!EvidenceLevel::Reachable.is_working());
    }

    #[test]
    fn only_a_fresh_high_pass_is_working() {
        let v = Vantage::runner_side();
        let mut evidence = Evidence::new();
        assert_eq!(evidence.status(&obfs4(), 1_000), Status::Unverified);
        evidence.record(Observation::new(
            Tier::T1,
            Outcome::Inconclusive,
            v.clone(),
            1_000,
            None,
            "timeout",
        ));
        assert_eq!(evidence.status(&obfs4(), 1_000), Status::Inconclusive);
        evidence.record(Observation::pass(
            Tier::T1,
            v.clone(),
            2_000,
            Some(30),
            "connected",
        ));
        assert_eq!(evidence.status(&obfs4(), 2_000), Status::Unverified);
        evidence.record(Observation::pass(
            Tier::T4,
            v.clone(),
            3_000,
            Some(8_000),
            "bootstrapped",
        ));
        assert_eq!(evidence.status(&obfs4(), 3_000), Status::Working);
        assert_eq!(
            evidence.status(&obfs4(), 3_000 + MAX_AGE_SECS),
            Status::Working
        );
        assert_eq!(
            evidence.status(&obfs4(), 3_000 + MAX_AGE_SECS + 1),
            Status::Stale
        );
        // A later inconclusive probe does not turn the pass into anything else.
        evidence.record(Observation::new(
            Tier::T4,
            Outcome::Inconclusive,
            v.clone(),
            4_000,
            None,
            "timeout",
        ));
        assert_eq!(evidence.status(&obfs4(), 4_000), Status::Working);
        // A later fail does.
        evidence.record(Observation::new(
            Tier::T1,
            Outcome::Fail,
            v.clone(),
            5_000,
            None,
            "connection refused",
        ));
        assert_eq!(evidence.status(&obfs4(), 5_000), Status::Failed);
        // And the pass before the fail still shows in the level.
        assert_eq!(evidence.level(&obfs4()), EvidenceLevel::Bootstrapped);
    }

    #[test]
    fn results_arriving_out_of_order_do_not_overwrite_newer_ones() {
        let v = Vantage::runner_side();
        let mut evidence = Evidence::new();
        evidence.record(Observation::new(
            Tier::T4,
            Outcome::Fail,
            v.clone(),
            500,
            None,
            "refused",
        ));
        evidence.record(Observation::pass(
            Tier::T4,
            v.clone(),
            400,
            None,
            "old pass",
        ));
        assert_eq!(evidence.decisive(Tier::T4).unwrap().detail, "refused");
        // The older pass is still the record of what was tried at 400.
        assert_eq!(evidence.last_attempt(Tier::T4).unwrap().detail, "refused");
        assert_eq!(evidence.status(&obfs4(), 600), Status::Failed);
        evidence.record(Observation::pass(
            Tier::T4,
            v.clone(),
            500,
            None,
            "same-second pass",
        ));
        assert_eq!(
            evidence.decisive(Tier::T4).unwrap().detail,
            "same-second pass"
        );
        assert_eq!(evidence.most_recent().unwrap().detail, "same-second pass");
        assert_eq!(evidence.all().count(), 1);
        // An inconclusive result is recorded as an attempt and decides nothing.
        evidence.record(Observation::new(
            Tier::T4,
            Outcome::Inconclusive,
            v,
            700,
            None,
            "timeout",
        ));
        assert_eq!(evidence.last_attempt(Tier::T4).unwrap().detail, "timeout");
        assert_eq!(
            evidence.decisive(Tier::T4).unwrap().detail,
            "same-second pass"
        );
        assert_eq!(evidence.status(&obfs4(), 700), Status::Working);
        assert_eq!(evidence.most_recent().unwrap().detail, "timeout");
    }

    #[test]
    fn fronted_transports_are_judged_on_tor_rungs_only() {
        let snowflake = Transport::Snowflake;
        let v = Vantage::runner_side();
        let mut evidence = Evidence::new();
        // Somebody recorded a TCP fail against the placeholder address: it decides nothing.
        evidence.record(Observation::new(
            Tier::T1,
            Outcome::Fail,
            v.clone(),
            10,
            None,
            "refused",
        ));
        assert_eq!(evidence.status(&snowflake, 20), Status::Unverified);
        evidence.record(Observation::new(
            Tier::T3,
            Outcome::Pass,
            v.clone(),
            20,
            Some(700),
            "assigned a snowflake",
        ));
        assert_eq!(evidence.status(&snowflake, 20), Status::Unverified);
        assert_eq!(
            evidence.level(&snowflake),
            EvidenceLevel::TransportHandshake
        );
        evidence.record(Observation::pass(
            Tier::T4,
            v,
            30,
            Some(4_000),
            "bootstrapped",
        ));
        assert_eq!(evidence.status(&snowflake, 30), Status::Working);
    }

    #[test]
    fn evidence_serializes() {
        let mut evidence = Evidence::new();
        evidence.record(Observation::pass(
            Tier::T4,
            Vantage::runner_side(),
            10,
            Some(1),
            "ok",
        ));
        let json = serde_json::to_string(&evidence).unwrap();
        assert!(json.contains("\"t4\""), "{json}");
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, evidence);
        let v: Vantage = serde_json::from_str(
            r#"{"label":"home, Iran","country":"IR","in_target_country":true}"#,
        )
        .unwrap();
        assert!(v.in_target_country);
    }
}
