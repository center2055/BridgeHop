//! Property tests: the parser is total, and whatever it accepts survives a round trip.
//!
//! Run with `cargo test -p hf-core`. To explore longer than the default case count:
//! `PROPTEST_CASES=200000 cargo test -p hf-core --test property`.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use hf_core::{parse_line, parse_lines, Bridge, Fingerprint, Transport};
use proptest::prelude::*;

// `any::<String>()` covers everything a Unicode string can hold: empty, huge, unpaired
// surrogates, RTL marks, NUL bytes, lines of only separators.

/// A line assembled from pieces a feed realistically contains, right or wrong.
fn realistic_line() -> impl Strategy<Value = String> {
    let transport = prop_oneof![
        Just(""),
        Just("obfs4 "),
        Just("webtunnel "),
        Just("snowflake "),
        Just("meek_lite "),
        Just("conjure "),
        Just("dnstt "),
        Just("vanilla "),
        Just("obfs3 "),
        Just("Bridge "),
        Just("bridge "),
    ];
    let host = prop_oneof![
        Just("1.2.3.4"),
        Just("203.0.113.9"),
        Just("127.0.0.1"),
        Just("[2a01:4f8::1]"),
        Just("[2001:db8::1]"),
        Just("::1"),
        Just("example.com"),
        Just("1.2.3"),
        Just("01.2.3.4"),
        Just("256.2.3.4"),
    ];
    let port = prop_oneof![
        Just(":443"),
        Just(":80"),
        Just(":0"),
        Just(":65536"),
        Just(":-1"),
        Just(""),
    ];
    let fingerprint = prop_oneof![
        Just(""),
        Just("0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
        Just("0024fbf8aa8ece3bac3e0c4383955c4bdd2b9866"),
        Just("0024FBF8"),
        Just("ZZZZFBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
    ];
    let arg = prop_oneof![
        Just(""),
        Just(" cert=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        Just(" cert=QUJD"),
        Just(" iat-mode=0"),
        Just(" iat-mode=2"),
        Just(" iat-mode="),
        Just(" url=https://a.example/x"),
        Just(" url=ftp://a.example/"),
        Just(" front=a.example,b.example"),
        Just(" fronts=a.example"),
        Just(" fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72"),
        Just(" doh=https://d.example/dns-query"),
        Just(" pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f"),
        Just(" domain=t.example"),
        Just(" bogus"),
        Just(" =x"),
        Just(" 9k=v"),
        Just(" k"),
        Just(" a\u{200b}=v"),
    ];
    (
        transport,
        host,
        port,
        fingerprint,
        prop::collection::vec(arg, 0..4),
        prop::bool::ANY,
        prop::bool::ANY,
    )
        .prop_map(|(t, h, p, fp, args, pad, bridge_kw)| {
            let mut line = String::new();
            if pad {
                line.push_str(" \t");
            }
            if bridge_kw && !t.is_empty() {
                line.push_str("Bridge ");
            }
            line.push_str(t);
            line.push_str(h);
            line.push_str(p);
            if !fp.is_empty() {
                line.push(' ');
                line.push_str(fp);
            }
            for a in args {
                line.push_str(a);
            }
            if pad {
                line.push_str("\r\n");
            }
            line
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn any_string_never_panics(line in any::<String>()) {
        let _ = parse_line(&line);
    }

    #[test]
    fn any_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let text = String::from_utf8_lossy(&bytes);
        let _ = parse_line(&text);
        let _ = parse_lines(text.lines());
    }

    #[test]
    fn accepted_lines_round_trip(line in realistic_line()) {
        let Ok(first) = parse_line(&line) else { return Ok(()) };
        let again = parse_line(&first.tor_line()).expect("tor_line() must re-parse");
        prop_assert_eq!(again.id(), first.id());
        prop_assert_eq!(&again.transport, &first.transport);
        prop_assert_eq!(&again.args, &first.args);
        prop_assert_eq!(&again.fingerprint, &first.fingerprint);
        prop_assert_eq!(again.id().len(), 64);
        prop_assert!(again.id().bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }

    #[test]
    fn rejected_lines_always_give_a_reason(line in realistic_line()) {
        if let Err(err) = parse_line(&line) {
            prop_assert!(!err.detail.is_empty());
        }
    }

    #[test]
    fn direct_transports_never_keep_a_non_public_address(line in realistic_line()) {
        let Ok(bridge) = parse_line(&line) else { return Ok(()) };
        if bridge.transport.is_direct() {
            prop_assert_eq!(hf_core::classify_addr(bridge.addr.ip()), hf_core::AddrClass::Public);
        }
    }

    #[test]
    fn reparsing_a_feed_is_idempotent(a in realistic_line(), b in realistic_line(), c in realistic_line()) {
        let first = parse_lines([a.clone(), b.clone(), c.clone()]);
        let again = parse_lines(first.bridges.iter().map(|bridge| bridge.raw.clone()));
        prop_assert_eq!(again.bridges.len(), first.bridges.len());
        prop_assert!(again.rejected.is_empty(), "{:?}", again.rejected);
        let ids: Vec<String> = again.bridges.iter().map(Bridge::id).collect();
        for pair in ids.windows(2) {
            prop_assert_ne!(&pair[0], &pair[1]);
        }
    }

    #[test]
    fn identity_is_order_and_spelling_independent(
        addr in prop_oneof![Just("1.2.3.4:443"), Just("[2a01:4f8::1]:443")],
        fp in prop_oneof![Just("0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"), Just("")],
        a in 0u8..5,
        b in 0u8..5,
        keyword in prop::bool::ANY,
    ) {
        let mut one = BTreeMap::new();
        one.insert(format!("k{a}"), format!("v{a}"));
        one.insert(format!("k{b}"), format!("v{b}"));
        let fp_parsed: Option<Fingerprint> =
            if fp.is_empty() { None } else { Some(fp.parse::<Fingerprint>().unwrap()) };
        let base = Bridge {
            transport: Transport::Unknown("xpt".into()),
            addr: addr.parse::<SocketAddr>().unwrap(),
            fingerprint: fp_parsed,
            args: one.clone(),
            raw: String::new(),
            issues: Vec::new(),
        };
        let mut shuffled = base.clone();
        let mut keys: Vec<String> = shuffled.args.keys().cloned().collect();
        keys.reverse();
        shuffled.args = keys.into_iter().filter_map(|k| one.get(&k).map(|v| (k, v.clone()))).collect();
        prop_assert_eq!(base.id(), shuffled.id());
        let text = if keyword { format!("Bridge {}", base.tor_line()) } else { base.tor_line() };
        let parsed = parse_line(&text).unwrap();
        prop_assert_eq!(parsed.id(), base.id());
    }
}
