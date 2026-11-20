use super::*;
use crate::cluster::enrollment::JoinAnchor;

// Independently computed with hashlib from the D3 field layout.
const ROOT: &[u8] = &[0x30, 3, 2, 1, 1];
const INNER_FIXTURE: &str = concat!(
    "arxj2-",
    "01010101010101010101010101010101",
    "02020202020202020202020202020202",
    "0303030303030303030303030303030303030303030303030303030303030303",
    "1b65f68a522c858715f5dd951cd0402dc16691778814bf0759822b7a257421d0",
    "0404040404040404040404040404040404040404040404040404040404040404",
    "e611c78e",
);

fn route(network: &str, prefix: u8) -> LocalRoute {
    LocalRoute {
        network: network.parse().unwrap(),
        prefix,
    }
}

fn raw_v4() -> Vec<u8> {
    let mut bytes = hex::decode("04c0a8012a234500053003020101").unwrap();
    bytes.extend(INNER_FIXTURE.bytes());
    bytes
}

fn wrap(mut body: Vec<u8>) -> String {
    body.extend(checksum(&body));
    format!("{PREFIX}{}", hex::encode(body))
}

#[test]
fn independent_ipv4_and_ipv6_envelopes_preserve_the_inner_code_and_root() {
    let code = JoinCode::parse(INNER_FIXTURE).unwrap();
    let inner_hex = hex::encode(INNER_FIXTURE);
    for (addr, routes, head, check) in [
        (
            "192.168.1.42:9029",
            [route("192.168.1.0", 24)],
            "04c0a8012a234500053003020101",
            "ff0a45ff",
        ),
        (
            "[fd7a:115c:a1e0::123]:9029",
            [route("fd7a:115c:a1e0::", 48)],
            "06fd7a115ca1e000000000000000000123234500053003020101",
            "e174c07f",
        ),
    ] {
        let addr = addr.parse().unwrap();
        let fixture = format!("{PREFIX}{head}{inner_hex}{check}");
        let encoded = encode(addr, ROOT, &code).unwrap();
        assert!(
            encoded.expose_for_owner_transfer() == fixture,
            "wire fixture"
        );
        let parsed = parse(&format!(" \t{fixture}\r\n"), &routes).unwrap();
        assert_eq!(parsed.endpoint, addr);
        assert_eq!(parsed.root, ROOT);
        assert!(parsed.code == code, "credential must survive byte-for-byte");
    }
}

#[test]
fn outer_syntax_checksum_length_and_eof_are_enforced() {
    let routes = [route("192.168.1.0", 24)];
    let raw = raw_v4();
    for end in 0..raw.len() {
        assert!(parse(&wrap(raw[..end].to_vec()), &routes).is_err());
    }
    let mut extra = raw.clone();
    extra.push(0);
    assert_eq!(parse(&wrap(extra), &routes).err(), Some(Refusal::Envelope));
    let mut family = raw.clone();
    family[0] = 5;
    assert_eq!(parse(&wrap(family), &routes).err(), Some(Refusal::Envelope));

    let valid = wrap(raw);
    let mut mistyped = valid.clone();
    mistyped.replace_range(mistyped.len() - 1.., "0");
    for malformed in [
        INNER_FIXTURE.to_owned(),
        valid[..valid.len() - 1].to_owned(),
        format!("{PREFIX}{}", valid[PREFIX.len()..].to_ascii_uppercase()),
        valid.replacen(PREFIX, "arxje2-", 1),
        format!("{valid}\u{a0}"),
        mistyped,
    ] {
        assert_eq!(parse(&malformed, &routes).err(), Some(Refusal::Envelope));
    }
}

#[test]
fn correct_outer_checksum_cannot_bypass_inner_canonicality_or_root_pin() {
    let routes = [route("192.168.1.0", 24)];
    let mut uppercase = raw_v4();
    uppercase[20..].make_ascii_uppercase();
    assert_eq!(
        parse(&wrap(uppercase), &routes).err(),
        Some(Refusal::Envelope)
    );
    let mut damaged_inner = raw_v4();
    *damaged_inner.last_mut().unwrap() = b'0';
    assert_eq!(
        parse(&wrap(damaged_inner), &routes).err(),
        Some(Refusal::Envelope)
    );
    let mut different_root = raw_v4();
    different_root[13] ^= 1;
    assert_eq!(
        parse(&wrap(different_root), &routes).err(),
        Some(Refusal::Root)
    );
    let code = JoinCode::parse(INNER_FIXTURE).unwrap();
    assert_eq!(
        encode("192.168.1.42:9029".parse().unwrap(), b"other", &code).err(),
        Some(Refusal::Root)
    );
}

#[test]
fn bounds_cover_original_root_and_raw_paste_with_whitespace() {
    let endpoint = "[fd7a:115c:a1e0::123]:9029".parse().unwrap();
    let routes = [route("fd7a:115c:a1e0::", 48)];
    let mut root = vec![1; MAX_ROOT];
    let code_for = |root: &[u8]| {
        JoinCode::generate([1; 16], fingerprint_of(root), JoinAnchor([4; 32]), |b| {
            b.fill(3);
            Ok::<_, ()>(())
        })
        .unwrap()
    };
    let empty = encode(endpoint, &[], &code_for(&[]));
    assert_eq!(empty.err(), Some(Refusal::Root));
    let mut text = encode(endpoint, &root, &code_for(&root)).unwrap().0;
    assert_eq!(text.len(), 8789);
    text.push_str(&" ".repeat(MAX_PASTE - text.len()));
    assert_eq!(parse(&text, &routes).unwrap().root, root);
    text.push(' ');
    assert_eq!(parse(&text, &routes).err(), Some(Refusal::Envelope));

    root.push(1);
    assert_eq!(
        encode(endpoint, &root, &code_for(&root)).err(),
        Some(Refusal::Root)
    );
    for length in [0u16, 4097] {
        let mut raw = raw_v4();
        raw[7..9].copy_from_slice(&length.to_be_bytes());
        assert_eq!(parse(&wrap(raw), &routes).err(), Some(Refusal::Envelope));
    }
}

#[test]
fn unsafe_endpoint_shapes_are_rejected_on_write_and_read() {
    let code = JoinCode::parse(INNER_FIXTURE).unwrap();
    let routes = [route("0.0.0.0", 1), route("::", 1)];
    for addr in [
        "0.0.0.0:9",
        "127.0.0.2:9",
        "224.1.1.1:9",
        "169.254.1.1:9",
        "255.255.255.255:9",
        "192.168.1.42:0",
        "[::]:9",
        "[::1]:9",
        "[ff02::42]:9",
        "[fe80::42]:9",
        "[::ffff:192.168.1.42]:9",
    ] {
        let addr: SocketAddr = addr.parse().unwrap();
        assert_eq!(encode(addr, ROOT, &code).err(), Some(Refusal::Route));
        let mut raw = match addr.ip() {
            IpAddr::V4(ip) => [vec![4], ip.octets().to_vec()].concat(),
            IpAddr::V6(ip) => [vec![6], ip.octets().to_vec()].concat(),
        };
        raw.extend(addr.port().to_be_bytes());
        raw.extend(&raw_v4()[7..]);
        assert_eq!(parse(&wrap(raw), &routes).err(), Some(Refusal::Route));
    }
    for addr in [
        "[fd7a:115c:a1e0::123%3]:9029".parse().unwrap(),
        std::net::SocketAddrV6::new("fd7a:115c:a1e0::123".parse().unwrap(), 9029, 1, 0).into(),
    ] {
        assert_eq!(encode(addr, ROOT, &code).err(), Some(Refusal::Route));
    }
}

#[test]
fn explicit_local_routes_exclude_default_external_and_directed_broadcast() {
    let code = JoinCode::parse(INNER_FIXTURE).unwrap();
    let routes = [route("192.168.0.0", 16), route("192.168.1.0", 24)];
    for addr in ["192.168.1.255:9", "192.168.1.0:9", "198.51.100.1:9"] {
        let paste = encode(addr.parse().unwrap(), ROOT, &code).unwrap();
        assert_eq!(
            parse(paste.expose_for_owner_transfer(), &routes).err(),
            Some(Refusal::Route)
        );
    }
    let tailnet = encode("100.64.2.3:9".parse().unwrap(), ROOT, &code).unwrap();
    let text = tailnet.expose_for_owner_transfer();
    for routes in [
        vec![],
        vec![route("0.0.0.0", 0)],
        vec![route("100.64.2.3", 10)],
    ] {
        assert_eq!(parse(text, &routes).err(), Some(Refusal::Route));
    }
    assert!(parse(text, &[route("100.64.0.0", 10)]).is_ok());
    assert!(parse(text, &[route("100.64.2.2", 31)]).is_ok());
    let ipv6 = encode("[2001:db8::1]:9".parse().unwrap(), ROOT, &code).unwrap();
    assert_eq!(
        parse(ipv6.expose_for_owner_transfer(), &[route("::", 0)]).err(),
        Some(Refusal::Route)
    );
}

#[test]
fn owner_paste_debug_is_redacted() {
    let code = JoinCode::parse(INNER_FIXTURE).unwrap();
    let paste = encode("192.168.1.42:9029".parse().unwrap(), ROOT, &code).unwrap();
    assert_eq!(format!("{paste:?}"), "<redacted>");
    assert_eq!(format!("{code:?}"), "<redacted>");
}
