//! Parser behaviour, exercised on real lines copied from the bridge collectors and on the
//! mistakes those collectors actually make.

use hf_core::{
    classify_addr, parse_line, parse_lines, AddrClass, Bridge, Fingerprint, Issue, ProbeTarget,
    Reject, Transport, Url, OBFS4_CERT_CHARS,
};

// Real lines, copied verbatim from the feeds (public bridge lines, 2026-11-27).
const OBFS4: &str = "obfs4 45.95.232.5:443 3CE205F7D7DE61E6E1787024EEDC03DCCFCA3C6A cert=1vxTSEr+6QCPpop3y0uEpY/V1ad4fB8ANayCPmh8DcPn4pt5kYNFctUjCA5A+GPmST1LNA iat-mode=2";
const WEBTUNNEL: &str = "webtunnel 85.215.163.16:443 775A648318DA1E1240EF0DEC038154B60696DE5B url=https://pod01.oneclickhost.eu/jLoIafiPpOBkJRJBN5mm6GPe ver=0.0.4";
const WEBTUNNEL_LOOPBACK: &str =
    "webtunnel 127.0.0.1:15000 D74CBD2AD903BD9502A6DAA6DCEC0C5A710AD686 url=https://tunnel.sandale.saarland/3981cf037e2f8fefec8a9ef06e440ef9";
const WEBTUNNEL_V6_DOC: &str = "webtunnel [2001:db8:1169:5d59:447d:1feb:3595:b174]:443 9E22636EA817AD4FE43F2EC14DEED5131FED1CB0 url=https://cdn-40.triplebit.dev/aif5ohWa4aWiephu ver=0.0.2";
const SNOWFLAKE: &str = "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://snowflake-broker.torproject.net.global.prod.fastly.net/ front=cdn.sstatic.net";
const SNOWFLAKE_FULL: &str = "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://1098762253.rsc.cdn77.org fronts=www.phpmyadmin.net,cdn.zk.mk ice=stun:stun.antisip.com:3478,stun:stun.epygi.com:3478 utls-imitate=hellorandomizedalpn";
const MEEK: &str = "meek_lite 192.0.2.21:80 url=https://1603026938.rsc.cdn77.org front=www.phpmyadmin.net utls=HelloRandomizedALPN";
const MEEK_AZURE: &str =
    "meek_lite 192.0.2.20:80 97700DFE9F483596DDA6264C4D7DF7641E1E39CE url=https://meek.azureedge.net/ front=ajax.aspnetcdn.com";
const CONJURE: &str = "conjure 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://registration.refraction.network/api fronts=cdn.sstatic.net,assets.cloud.censys.io transport=min";
const VANILLA: &str = "96.9.98.148:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866";
// OnionHop's own test fixture for the community `dnstt` convention.
const DNSTT: &str = "dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE doh=https://dns.google/dns-query pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f domain=t.ruhnama.net";
const CERT: &str = "1vxTSEr+6QCPpop3y0uEpY/V1ad4fB8ANayCPmh8DcPn4pt5kYNFctUjCA5A+GPmST1LNA";

fn ok(line: &str) -> Bridge {
    parse_line(line).unwrap_or_else(|e| panic!("{line:?}: {e}"))
}

fn why(line: &str) -> Reject {
    parse_line(line)
        .err()
        .unwrap_or_else(|| panic!("{line:?} parsed"))
        .reason
}

#[test]
fn real_feed_lines_parse_cleanly() {
    let b = ok(OBFS4);
    assert_eq!(b.transport, Transport::Obfs4);
    assert_eq!(b.addr.to_string(), "45.95.232.5:443");
    assert_eq!(
        b.fingerprint.map(|f| f.to_hex()),
        Some("3CE205F7D7DE61E6E1787024EEDC03DCCFCA3C6A".into())
    );
    assert_eq!(b.arg("cert"), Some(CERT));
    assert_eq!(b.arg("iat-mode"), Some("2"));
    assert!(b.issues.is_empty(), "{:?}", b.issues);
    assert_eq!(
        b.probe_target(),
        Some(ProbeTarget::Addr("45.95.232.5:443".parse().unwrap()))
    );

    let b = ok(WEBTUNNEL);
    assert_eq!(b.transport, Transport::Webtunnel);
    assert!(b.issues.is_empty(), "{:?}", b.issues);
    assert_eq!(
        b.probe_target(),
        Some(ProbeTarget::Host {
            host: "pod01.oneclickhost.eu".into(),
            port: 443,
            server_name: "pod01.oneclickhost.eu".into()
        })
    );

    // The line address is a placeholder for webtunnel; probing goes to the URL host.
    for (line, host) in [
        (WEBTUNNEL_LOOPBACK, "tunnel.sandale.saarland"),
        (WEBTUNNEL_V6_DOC, "cdn-40.triplebit.dev"),
    ] {
        let b = ok(line);
        assert_eq!(
            b.probe_target(),
            Some(ProbeTarget::Host {
                host: host.into(),
                port: 443,
                server_name: host.into()
            })
        );
        assert!(b.issues.is_empty(), "{:?}", b.issues);
    }

    let b = ok(SNOWFLAKE);
    assert_eq!(b.transport, Transport::Snowflake);
    assert_eq!(b.front_hosts(), vec!["cdn.sstatic.net".to_string()]);
    assert!(b.issues.is_empty(), "{:?}", b.issues);
    assert_eq!(b.probe_target(), None);

    let b = ok(SNOWFLAKE_FULL);
    assert!(b.issues.is_empty(), "{:?}", b.issues);
    assert_eq!(
        b.front_hosts(),
        vec!["www.phpmyadmin.net".to_string(), "cdn.zk.mk".to_string()]
    );

    let b = ok(MEEK);
    assert_eq!(b.transport, Transport::MeekLite);
    assert_eq!(b.issues, vec![Issue::MissingFingerprint]);
    assert_eq!(Transport::MeekLite.sidecar(), Some("lyrebird"));

    let b = ok(MEEK_AZURE);
    assert!(b.issues.is_empty(), "{:?}", b.issues);

    let b = ok(CONJURE);
    assert_eq!(b.transport, Transport::Conjure);
    assert!(b.issues.is_empty(), "{:?}", b.issues);

    let b = ok(VANILLA);
    assert_eq!(b.transport, Transport::Vanilla);
    assert!(b.issues.is_empty(), "{:?}", b.issues);

    let b = ok(DNSTT);
    assert_eq!(b.transport, Transport::Dnstt);
    assert!(b.issues.is_empty(), "{:?}", b.issues);
}

#[test]
fn bridge_keyword_and_case_are_normalized() {
    let a = ok(&format!("Bridge {VANILLA}"));
    let b = ok(VANILLA);
    let c = ok("96.9.98.148:443 0024fbf8aa8ece3bac3e0c4383955c4bdd2b9866");
    assert_eq!(a.id(), b.id());
    assert_eq!(b.id(), c.id());
    assert_eq!(a.tor_line(), format!("Bridge {VANILLA}"));
    assert_eq!(a.raw, format!("Bridge {VANILLA}"));
    // Argument keys are case-folded; values are not.
    let upper = ok(&format!(
        "obfs4 45.95.232.5:443 3CE205F7D7DE61E6E1787024EEDC03DCCFCA3C6A CERT={CERT} IAT-MODE=2"
    ));
    assert_eq!(upper.id(), ok(OBFS4).id());
    assert_eq!(why(&OBFS4.replace("cert=", "cert=x")), Reject::BadCert);
}

#[test]
fn identity_ignores_what_does_not_change_the_bridge() {
    // Argument order.
    let a = ok(&format!(
        "obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 cert={CERT} iat-mode=0"
    ));
    let b = ok(&format!(
        "obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 iat-mode=0 cert={CERT}"
    ));
    assert_eq!(a.id(), b.id());
    // iat-mode is a client-side option: the same bridge either way.
    let c = ok(&format!(
        "obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 cert={CERT} iat-mode=1"
    ));
    assert_eq!(a.endpoint_key(), c.endpoint_key());
    // A different certificate is a different bridge.
    let other = ok(&format!("obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 cert=ayq0XzCwhpdysn5o0EyDUbmSOx3X/oTEbzDMvczHOdBJKlvIdHHLJGkZARtT4dcBFArPPg iat-mode=0"));
    assert_ne!(a.endpoint_key(), other.endpoint_key());
    // The placeholder address on webtunnel lines and the `ver` the client never reads.
    let w1 = ok(WEBTUNNEL_LOOPBACK);
    let w2 = ok(&WEBTUNNEL_LOOPBACK.replacen("127.0.0.1:15000", "[2001:db8::1]:1", 1));
    assert_eq!(w1.endpoint_key(), w2.endpoint_key());
    assert_eq!(
        ok(WEBTUNNEL).endpoint_key(),
        ok(&WEBTUNNEL.replace("ver=0.0.4", "ver=0.0.7")).endpoint_key()
    );
    // The address does count for obfs4.
    assert_ne!(
        a.endpoint_key(),
        ok(&format!(
            "obfs4 1.2.3.5:443 0000000000000000000000000000000000000001 cert={CERT} iat-mode=0"
        ))
        .endpoint_key()
    );
    // `front=a,b` and `fronts=b, a` are one endpoint; utls and max are client-side.
    let s1 = ok("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://b.example/ front=a.example,b.example");
    let s2 = ok("snowflake 192.0.2.9:1 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fronts=b.example,a.example utls-imitate=none url=https://b.example/");
    assert_eq!(s1.endpoint_key(), s2.endpoint_key());
    // The fingerprint completes the identity.
    let mut pinned = ok(MEEK);
    let unpinned = pinned.id();
    pinned.fingerprint = Some("97700DFE9F483596DDA6264C4D7DF7641E1E39CE".parse().unwrap());
    assert_ne!(unpinned, pinned.id());
    assert_eq!(a.short_id().len(), 12);
    assert_eq!(a.id().len(), 64);
}

#[test]
fn ipv6_endpoints() {
    let b =
        ok(&format!("Bridge {WEBTUNNEL}").replace("85.215.163.16:443", "[2a01:4f8:1c17::1]:443"));
    assert_eq!(b.addr.to_string(), "[2a01:4f8:1c17::1]:443");
    assert_eq!(b.issues.is_empty(), true);
    let vanilla6 = ok("[2a01:4f8:1c17::1]:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866");
    assert_eq!(vanilla6.transport, Transport::Vanilla);
    assert_eq!(
        vanilla6.probe_target(),
        Some(ProbeTarget::Addr("[2a01:4f8:1c17::1]:443".parse().unwrap()))
    );
    assert_eq!(
        why("2a01:4f8:1c17::1:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
        Reject::BadAddress
    );
    assert_eq!(
        why("[2a01:4f8:1c17::1:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
        Reject::BadAddress
    );
    assert_eq!(
        why("[2a01:4f8:1c17::1 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
        Reject::BadAddress
    );
    assert_eq!(
        why("[2a01:4f8:qq::1]:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
        Reject::BadAddress
    );
    // A v4-mapped address is a valid public endpoint.
    assert!(
        ok("[::ffff:1.2.3.4]:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866")
            .issues
            .is_empty()
    );
}

#[test]
fn transports_that_dial_the_line_address_reject_placeholders() {
    for (line, reason) in [
        ("obfs4 127.0.0.1:15000 D74CBD2AD903BD9502A6DAA6DCEC0C5A710AD686 cert=DKA6xS0N6c4I1CFVmWc/qgAEhPBgR/dPQyXhbkjt082Ka2xmq1pwSUpCPt626uuKW2Z1TA iat-mode=0", Reject::NonRoutable),
        ("10.0.0.1:9001 0000000000000000000000000000000000000001", Reject::NonRoutable),
        ("[::1]:9001 0000000000000000000000000000000000000001", Reject::NonRoutable),
        ("0.0.0.0:9001 0000000000000000000000000000000000000001", Reject::NonRoutable),
        ("[fc00::1]:9001 0000000000000000000000000000000000000001", Reject::NonRoutable),
        ("192.0.2.1:443 0000000000000000000000000000000000000001", Reject::Placeholder),
        ("203.0.113.5:443 0000000000000000000000000000000000000001", Reject::Placeholder),
        ("[2001:db8::1]:443 0000000000000000000000000000000000000001", Reject::Placeholder),
    ] {
        assert_eq!(why(line), reason, "{line}");
    }
    assert_eq!(
        classify_addr("100.8.170.63".parse().unwrap()),
        AddrClass::Public
    );
    for cgnat in ["100.64.0.1", "100.127.255.1"] {
        assert_eq!(
            classify_addr(cgnat.parse().unwrap()),
            AddrClass::Private,
            "{cgnat}"
        )
    }
}

#[test]
fn bad_addresses_and_ports() {
    for (line, reason) in [
        (
            "256.1.1.1:443 0000000000000000000000000000000000000001",
            Reject::HostnameNotAllowed,
        ),
        (
            "1.2.3:443 0000000000000000000000000000000000000001",
            Reject::HostnameNotAllowed,
        ),
        (
            "01.2.3.4:443 0000000000000000000000000000000000000001",
            Reject::HostnameNotAllowed,
        ),
        (
            "1.2.3.4.5:443 0000000000000000000000000000000000000001",
            Reject::HostnameNotAllowed,
        ),
        (
            "bridge.example.com:443 0000000000000000000000000000000000000001",
            Reject::HostnameNotAllowed,
        ),
        (
            "1.2.3.4:0 0000000000000000000000000000000000000001",
            Reject::BadPort,
        ),
        (
            "1.2.3.4:65536 0000000000000000000000000000000000000001",
            Reject::BadPort,
        ),
        (
            "1.2.3.4:44a 0000000000000000000000000000000000000001",
            Reject::BadPort,
        ),
        (
            "1.2.3.4:+443 0000000000000000000000000000000000000001",
            Reject::BadPort,
        ),
        (
            "1.2.3.4 0000000000000000000000000000000000000001",
            Reject::BadAddress,
        ),
        ("1.2.3.4:443", Reject::MissingEndpoint),
        (
            "1.2.3.4:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866 unexpected",
            Reject::UnexpectedToken,
        ),
        (
            "1.2.3.4:443 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9867",
            Reject::UnexpectedToken,
        ),
        ("Bridge", Reject::MissingEndpoint),
        ("obfs4", Reject::MissingEndpoint),
        ("9obfs4 1.2.3.4:443", Reject::BadTransportName),
        ("obfs4 x 1.2.3.4:443", Reject::BadTransportName),
    ] {
        assert_eq!(why(line), reason, "{line}");
    }
    // A vanilla line needs its fingerprint: without it the address is all there is.
    assert_eq!(ok("1.2.3.4:443").issues, vec![Issue::MissingFingerprint]);
}

#[test]
fn obfs4_rules_follow_lyrebird() {
    let base = "obfs4 1.2.3.4:443 0000000000000000000000000000000000000001";
    // No cert and no legacy pair.
    assert_eq!(
        why(&format!("{base} iat-mode=0")),
        Reject::MissingRequiredArg
    );
    assert_eq!(
        why(&format!(
            "{base} iat-mode=0 node-id=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        )),
        Reject::MissingRequiredArg
    );
    assert_eq!(
        why(&format!("{base} node-id=zz public-key=zz iat-mode=0")),
        Reject::BadCert
    );
    // iat-mode is required by lyrebird and repaired to 0.
    let repaired = ok(&format!("{base} cert={CERT}"));
    assert_eq!(repaired.arg("iat-mode"), Some("0"));
    assert!(repaired.issues.contains(&Issue::MissingIatMode));
    assert_eq!(repaired.tor_line(), format!("Bridge obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 cert={CERT} iat-mode=0"));
    assert!(!ok(&repaired.tor_line())
        .issues
        .contains(&Issue::MissingIatMode));
    assert_eq!(
        why(&format!("{base} cert={CERT} iat-mode=3")),
        Reject::BadArg
    );
    assert_eq!(
        why(&format!("{base} cert={CERT} iat-mode=")),
        Reject::BadArg
    );
    // A cert must be exactly what lyrebird decodes: 70 standard-base64 characters for 52 bytes.
    assert_eq!(CERT.len(), OBFS4_CERT_CHARS);
    assert_eq!(
        why(&format!("{base} cert=QUJD iat-mode=0")),
        Reject::BadCert
    );
    assert_eq!(
        why(&format!("{base} cert=QUJD== iat-mode=0")),
        Reject::BadCert
    );
    assert_eq!(
        why(&format!(
            "{base} cert={} iat-mode=0",
            CERT.replace('+', "-")
        )),
        Reject::BadCert
    );
    assert_eq!(
        why(&format!("{base} cert={} iat-mode=0", &CERT[..69])),
        Reject::BadCert
    );
    assert_eq!(
        why(&format!("{base} cert={} iat-mode=0", format!("{CERT}A"))),
        Reject::BadCert
    );
    // The legacy pair is 40 + 64 hex characters.
    let legacy = ok(&format!("{base} node-id=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA public-key=0000000000000000000000000000000000000000000000000000000000000000 iat-mode=0"));
    assert!(legacy.issues.is_empty(), "{:?}", legacy.issues);
    assert_eq!(
        why(&format!(
            "{base} node-id=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA public-key=00 iat-mode=0"
        )),
        Reject::BadCert
    );
}

#[test]
fn url_transports_need_a_usable_url() {
    let fp = "0000000000000000000000000000000000000001";
    assert_eq!(
        why("webtunnel 85.215.163.16:443 775A648318DA1E1240EF0DEC038154B60696DE5B"),
        Reject::MissingRequiredArg
    );
    for bad in [
        "ftp://x.example/",
        "x.example/",
        "https:///",
        "https:///path",
        "https://host:0/",
        "https://host:99999/",
        "https://[::1",
        "https://user:pw@host.example/",
    ] {
        assert_eq!(
            why(&format!("webtunnel 85.215.163.16:443 {fp} url={bad}")),
            Reject::BadArg,
            "{bad}"
        );
    }
    assert_eq!(
        why("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72"),
        Reject::MissingRequiredArg
    );
    assert_eq!(why("meek_lite 192.0.2.20:80"), Reject::MissingRequiredArg);
    assert_eq!(
        why(&format!("conjure 192.0.2.3:80 {fp} url=ftp://x/")),
        Reject::BadArg
    );
    // Conjure works with client defaults, so nothing is required.
    assert_eq!(
        ok(&format!("conjure 192.0.2.3:80 {fp}")).issues,
        vec![Issue::NoFront]
    );
    // Snowflake without a front reaches the broker directly.
    assert!(ok("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://broker.example/").issues.contains(&Issue::NoFront));
    // A `fingerprint=` that disagrees with the line is reported, one that agrees is not.
    let agree = ok("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2b280b23e1107bb62abfc40ddcc8824814f80a72 url=https://broker.example/ front=a.example");
    assert!(agree.issues.is_empty(), "{:?}", agree.issues);
    let mismatch = ok("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=8838024498816A039FCBBAB14E6F40A0843051FA url=https://broker.example/ front=a.example");
    assert!(mismatch.issues.contains(&Issue::FingerprintMismatch));
    assert_eq!(why("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=zz url=https://broker.example/"), Reject::BadArg);
    assert_eq!(why("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 max=0 url=https://broker.example/"), Reject::BadArg);
}

#[test]
fn url_pieces() {
    let u = Url::parse("https://pod01.oneclickhost.eu/jLoIafiPpOBkJRJBN5mm6GPe?x=1#f").unwrap();
    assert!(u.secure);
    assert_eq!(u.host, "pod01.oneclickhost.eu");
    assert_eq!(u.port, 443);
    assert_eq!(u.path, "/jLoIafiPpOBkJRJBN5mm6GPe");
    let u = Url::parse("http://1098762253.rsc.cdn77.org:8080").unwrap();
    assert!(!u.secure);
    assert_eq!(u.port, 8080);
    assert_eq!(u.path, "");
    let u = Url::parse("https://[2001:db8::1]:8443/a/b").unwrap();
    assert_eq!(u.host, "2001:db8::1");
    assert_eq!(u.port, 8443);
    // webtunnel sends `servername` in the TLS handshake but still dials the URL host.
    let b = ok(&format!("{WEBTUNNEL} servername=www.example.org"));
    assert_eq!(
        b.probe_target(),
        Some(ProbeTarget::Host {
            host: "pod01.oneclickhost.eu".into(),
            port: 443,
            server_name: "www.example.org".into()
        })
    );
}

#[test]
fn dnstt_convention() {
    assert_eq!(why("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f domain=t.example"), Reject::MissingRequiredArg);
    assert_eq!(why("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE doh=https://d.example/dns-query pubkey=zz domain=t.example"), Reject::BadArg);
    assert_eq!(why("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE doh=https://d.example/dns-query domain=t.example"), Reject::MissingRequiredArg);
    assert_eq!(why("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE doh=not-a-url pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f domain=t.example"), Reject::BadArg);
    assert_eq!(why("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE dot=dot.example pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f domain=t.example"), Reject::BadArg);
    assert!(ok("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE dot=dot.example:853 pubkey=241169008830694749fe96bb070c4855c5bb5b9c47b3833ed7d88521ba30a43f domain=t.example").issues.is_empty());
    assert!(ok("dnstt 192.0.2.4:1 A998F319ADB60EE344540EC4B21524CC484F96BE udp=1.1.1.1:53 pubkey-file=server.pub domain=t.example").issues.is_empty());
}

#[test]
fn unknown_transports_are_kept_but_flagged() {
    let old = ok("obfs3 1.2.3.4:443 0000000000000000000000000000000000000001");
    assert_eq!(old.transport, Transport::Unknown("obfs3".into()));
    assert_eq!(old.issues, vec![Issue::UnsupportedTransport]);
    assert!(!old.transport.is_dialable());
    assert_eq!(
        old.probe_target(),
        Some(ProbeTarget::Addr("1.2.3.4:443".parse().unwrap()))
    );
    assert_eq!(
        ok("vanilla 1.2.3.4:443 0000000000000000000000000000000000000001").transport,
        Transport::Vanilla
    );
    // Vanilla takes no arguments.
    assert_eq!(
        ok(&format!("{VANILLA} k=v")).issues,
        vec![Issue::UnknownArg]
    );
    // Unknown arguments on known transports pass through with a note.
    assert!(ok(&format!("{WEBTUNNEL} extra=1"))
        .issues
        .contains(&Issue::UnknownArg));
}

#[test]
fn malformed_lines() {
    for (line, reason) in [
        ("", Reject::Empty),
        ("# comment", Reject::Empty),
        ("   \r\n", Reject::Empty),
        ("\u{feff}\u{200b}", Reject::Empty),
        (&"a".repeat(hf_core::MAX_LINE_LEN + 1), Reject::TooLong),
        (&format!("{VANILLA} k"), Reject::UnexpectedToken),
        (&format!("{VANILLA} =v"), Reject::BadArg),
        (&format!("{VANILLA} 1k=v"), Reject::BadArg),
        (&format!("{VANILLA} k="), Reject::BadArg),
        (
            &format!(
                "{VANILLA} k={} ",
                "x".repeat(hf_core::MAX_ARG_VALUE_LEN + 1)
            ),
            Reject::BadArg,
        ),
        (&format!("{VANILLA} k=v a"), Reject::UnexpectedToken),
        (
            &format!("96.9.98.148:443\u{200b} 0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866"),
            Reject::BadArg,
        ),
        (&format!("{VANILLA} k=v\u{7f}"), Reject::BadArg),
        (
            &format!("{VANILLA} k=v\tunexpected"),
            Reject::UnexpectedToken,
        ),
        ("garbage line here", Reject::BadAddress),
    ] {
        assert_eq!(why(line), reason, "{line:?}");
    }
    // Trailing whitespace is fine; the line parses.
    assert_eq!(ok(&format!("{VANILLA}  \t")).id(), ok(VANILLA).id());
    // A full-length snowflake line with a STUN list and more is still far below the limit.
    assert!(SNOWFLAKE_FULL.len() < 600);
    let many: String = (0..=hf_core::MAX_ARGS)
        .map(|i| format!(" k{i}=v"))
        .collect();
    assert_eq!(why(&format!("snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://b.example/{many}")), Reject::TooManyArgs);
    assert_eq!(
        why(&format!("{VANILLA} {}", "A".repeat(39))),
        Reject::UnexpectedToken
    );
}

#[test]
fn duplicates_keep_the_first_value_like_the_client() {
    let b = ok(&format!("{VANILLA} a=1 a=2"));
    assert_eq!(b.issues, vec![Issue::UnknownArg, Issue::DuplicateArg]);
    assert_eq!(b.arg("a"), Some("1"));
    let b = ok(&format!("obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 cert={CERT} cert={CERT} iat-mode=0"));
    assert_eq!(b.issues, vec![Issue::DuplicateArg]);
}

#[test]
fn parse_lines_merges_repeats_and_collects_rejections() {
    let set = parse_lines([
        VANILLA,
        "",
        "# profile-title: 🧅 TOR BRIDGES 🧅 Webtunnel",
        &format!("  Bridge  {VANILLA}  "),
        OBFS4,
        "garbage line here",
        "192.0.2.1:443 0000000000000000000000000000000000000001",
        OBFS4,
        // The same webtunnel bridge printed with another placeholder address.
        WEBTUNNEL_LOOPBACK,
        &WEBTUNNEL_LOOPBACK.replace("127.0.0.1:15000", "[2001:db8::5]:443"),
        // The same obfs4 bridge with and without iat-mode: merged, and the repair is kept.
        &format!(
            "obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 iat-mode=2 cert={CERT}"
        ),
        &format!("obfs4 1.2.3.4:443 0000000000000000000000000000000000000001 cert={CERT}"),
        // A vanilla bridge first seen without, then with its fingerprint: one bridge.
        "64.65.2.52:443",
        "64.65.2.52:443 0844B7152D17D88FD995FE333E1030906D2A5800",
        // The same endpoint pinned to two different relays: two bridges.
        "96.9.98.147:443 009FE3F95E21534F97D9091C188647B2F5E7B8FF",
        "96.9.98.147:443 8838024498816A039FCBBAB14E6F40A0843051FA",
    ]);
    let summary: Vec<(Transport, String)> = set
        .bridges
        .iter()
        .map(|b| (b.transport.clone(), b.addr.to_string()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (Transport::Vanilla, "96.9.98.148:443".into()),
            (Transport::Obfs4, "45.95.232.5:443".into()),
            (Transport::Webtunnel, "127.0.0.1:15000".into()),
            (Transport::Obfs4, "1.2.3.4:443".into()),
            (Transport::Vanilla, "64.65.2.52:443".into()),
            (Transport::Vanilla, "96.9.98.147:443".into()),
            (Transport::Vanilla, "96.9.98.147:443".into()),
        ]
    );
    assert_eq!(set.duplicates, 5);
    assert_eq!(
        set.rejected.iter().map(|r| r.reason).collect::<Vec<_>>(),
        vec![Reject::BadAddress, Reject::Placeholder]
    );
    assert_eq!(set.rejected[0].line_no, 6);
    assert_eq!(set.rejected[0].line, "garbage line here");
    let merged_obfs4 = &set.bridges[3];
    assert_eq!(merged_obfs4.arg("iat-mode"), Some("2"));
    assert!(!merged_obfs4.issues.contains(&Issue::MissingIatMode));
    let merged_vanilla = &set.bridges[4];
    assert_eq!(
        merged_vanilla.fingerprint.map(|f| f.to_hex()),
        Some("0844B7152D17D88FD995FE333E1030906D2A5800".into())
    );
    assert!(
        merged_vanilla.issues.is_empty(),
        "{:?}",
        merged_vanilla.issues
    );
    assert_eq!(
        merged_vanilla.tor_line(),
        "Bridge 64.65.2.52:443 0844B7152D17D88FD995FE333E1030906D2A5800"
    );
    let ids: std::collections::HashSet<String> = set.bridges.iter().map(Bridge::id).collect();
    assert_eq!(
        ids.len(),
        set.bridges.len(),
        "every kept bridge has a distinct identity"
    );
}

#[test]
fn a_feed_that_repeats_lines_is_stable_under_reprocessing() {
    let feed = [
        VANILLA,
        OBFS4,
        WEBTUNNEL,
        SNOWFLAKE_FULL,
        MEEK,
        CONJURE,
        DNSTT,
    ];
    let twice = feed.iter().chain(feed.iter()).copied();
    let set = parse_lines(twice);
    assert_eq!(set.bridges.len(), feed.len());
    assert_eq!(set.duplicates, feed.len());
    let again = parse_lines(set.bridges.iter().map(|b| b.tor_line()));
    assert_eq!(again.bridges.len(), set.bridges.len());
    assert!(again.rejected.is_empty(), "{:?}", again.rejected);
    for (a, b) in set.bridges.iter().zip(&again.bridges) {
        assert_eq!(a.id(), b.id());
        assert_eq!(a.probe_target(), b.probe_target());
    }
}

#[test]
fn address_classes() {
    for (ip, class) in [
        ("8.8.8.8", AddrClass::Public),
        ("192.0.2.9", AddrClass::Documentation),
        ("127.0.0.1", AddrClass::Loopback),
        ("192.168.1.1", AddrClass::Private),
        ("100.64.0.1", AddrClass::Private),
        ("100.8.170.63", AddrClass::Public),
        ("100.127.255.1", AddrClass::Private),
        ("169.254.1.1", AddrClass::Private),
        ("0.1.2.3", AddrClass::Reserved),
        ("224.0.0.1", AddrClass::Reserved),
        ("255.255.255.255", AddrClass::Reserved),
        ("2606:4700::1", AddrClass::Public),
        ("2a01:4f8:1c17::1", AddrClass::Public),
        ("2001:db8::1", AddrClass::Documentation),
        ("fc00::1", AddrClass::Private),
        ("fe80::1", AddrClass::Private),
        ("::1", AddrClass::Loopback),
        ("::", AddrClass::Reserved),
        ("ff02::1", AddrClass::Reserved),
        ("::ffff:192.0.2.1", AddrClass::Documentation),
    ] {
        assert_eq!(classify_addr(ip.parse().unwrap()), class, "{ip}");
    }
}

#[test]
fn fingerprint_type() {
    let fp: Fingerprint = "0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866".parse().unwrap();
    assert_eq!(fp.to_hex(), "0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866");
    assert_eq!(fp.as_bytes()[0], 0x00);
    assert_eq!(format!("{fp}"), "0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866");
    assert_eq!(
        format!("{fp:?}"),
        "Fingerprint(0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866)"
    );
    assert_eq!(
        serde_json::to_string(&fp).unwrap(),
        "\"0024FBF8AA8ECE3BAC3E0C4383955C4BDD2B9866\""
    );
    assert_eq!(fp, fp);
    assert!("zz".parse::<Fingerprint>().is_err());
}

#[test]
fn bridge_json_round_trip() {
    for line in [
        OBFS4,
        WEBTUNNEL_LOOPBACK,
        SNOWFLAKE_FULL,
        MEEK,
        CONJURE,
        VANILLA,
        DNSTT,
    ] {
        let b = ok(line);
        let json = serde_json::to_string(&b).unwrap();
        let back: Bridge = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
        assert_eq!(back.id(), b.id());
    }
}
