//! HopForge core: the vocabulary everything else is written in.
//!
//! This crate holds no I/O and no async runtime. It defines what a bridge line is
//! ([`parse`]), which transport a line names ([`transport`]), and what it means for a bridge to
//! have been verified ([`ladder`]). The probe, source, fetch, tunnel and decision crates all
//! speak these types, so a result can travel from a feed file to the UI without re-parsing.
//!
//! Nothing in here trusts its input: every public function is total over `&str`, and the
//! property tests in `tests/` feed arbitrary strings and arbitrary bytes to the parser.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod ladder;
pub mod parse;
pub mod transport;

pub use ladder::{
    Evidence, EvidenceLevel, Observation, Outcome, Status, Tier, Vantage, MAX_AGE_SECS,
    MIN_WORKING_TIER,
};
pub use parse::{
    classify_addr, parse_line, parse_lines, AddrClass, Bridge, Fingerprint, Issue, ParseError,
    ParsedSet, ProbeTarget, Reject, Rejected, Url, MAX_ARGS, MAX_ARG_VALUE_LEN, MAX_LINE_LEN,
    OBFS4_CERT_CHARS, OBFS4_CERT_LEN,
};
pub use transport::{DialTarget, Transport, MAX_TRANSPORT_NAME_LEN};
