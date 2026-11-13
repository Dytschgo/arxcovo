//! Bounded parsers; no filesystem, process or environment observations.
use crate::service::{config::Rights, preflight::ExecutionUnavailable};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};

pub(crate) struct Location {
    unit: Vec<String>,
}
impl Location {
    pub(crate) fn unit_components(&self) -> &[String] {
        &self.unit
    }
    pub(crate) fn runs(&self) -> PathBuf {
        let mut path = PathBuf::from("/sys/fs/cgroup");
        for component in &self.unit {
            path.push(component);
        }
        path.push("runs");
        path
    }
}
pub(crate) fn locate(bytes: &[u8]) -> Option<Location> {
    if bytes.len() > 4096 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let path = text.strip_prefix("0::/")?;
    if path.contains(['\0', '\\', '\n', '\r']) {
        return None;
    }
    let mut parts: Vec<_> = path.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
        || parts.pop()? != "supervisor"
        || parts.last().copied()? != "arxd.service"
    {
        return None;
    }
    Some(Location {
        unit: parts.into_iter().map(str::to_owned).collect(),
    })
}
fn decimal(bytes: &[u8], max: usize) -> bool {
    !bytes.is_empty()
        && bytes.len() <= max
        && bytes.iter().all(u8::is_ascii_digit)
        && (bytes.len() == 1 || bytes.first() != Some(&b'0'))
}
pub(crate) fn leaf_name(bytes: &[u8]) -> bool {
    if bytes.len() > 35 {
        return false;
    }
    let Some(rest) = bytes.strip_prefix(b"arx-") else {
        return false;
    };
    let mut fields = rest.split(|b| *b == b'-');
    matches!((fields.next(), fields.next(), fields.next()), (Some(pid), Some(serial), None) if decimal(pid, 10) && decimal(serial, 20))
}
pub(crate) fn events(text: &str) -> Option<bool> {
    if text.len() > 4096 {
        return None;
    }
    let mut keys = BTreeSet::new();
    let mut populated = None;
    for line in text.strip_suffix('\n').unwrap_or(text).split('\n') {
        let (key, value) = line.split_once(' ')?;
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            || value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || !keys.insert(key)
        {
            return None;
        }
        if key == "populated" {
            populated = Some(match value {
                "0" => false,
                "1" => true,
                _ => return None,
            });
        }
    }
    populated
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Dot,
    Child,
    Control,
    Refused,
}
// Linux dirent d_type and stat mode ABI values, represented as plain data on
// every OS. E3 will supply these values; E1 never calls libc or fstatat.
pub(crate) fn classify_entry(name: &[u8], d_type: u8, mode: Option<Result<u32, ()>>) -> Entry {
    if name == b"." || name == b".." {
        return Entry::Dot;
    }
    if name.is_empty() || name.len() > 255 || name.iter().any(|b| b"/\\\0".contains(b)) {
        return Entry::Refused;
    }
    let kind = match d_type {
        0 => match mode {
            Some(Ok(m)) => match m & 0o170000 {
                0o040000 => 4,
                0o100000 => 8,
                _ => 255,
            },
            _ => 255,
        },
        other => other,
    };
    match kind {
        4 => Entry::Child,
        8 if name.len() <= 64
            && name.contains(&b'.')
            && name.split(|b| *b == b'.').all(|part| {
                !part.is_empty()
                    && part
                        .iter()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
            }) =>
        {
            Entry::Control
        }
        _ => Entry::Refused,
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Controllers {
    pub(crate) memory: bool,
    pub(crate) pids: bool,
}
pub(crate) fn controllers(text: &str) -> Controllers {
    let mut result = Controllers {
        memory: false,
        pids: false,
    };
    if text.len() <= 4096 {
        for token in text.split_whitespace() {
            match token {
                "memory" => result.memory = true,
                "pids" => result.pids = true,
                _ => {}
            }
        }
    }
    result
}
pub(crate) fn pids(text: &str) -> Option<Vec<u32>> {
    if text.len() > 4096 {
        return None;
    }
    if text.is_empty() {
        return Some(Vec::new());
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .map(|line| {
            if !decimal(line.as_bytes(), 10) {
                return None;
            }
            line.parse().ok()
        })
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hardening {
    Standard,
    None,
}
pub(crate) fn hardening(bytes: &[u8]) -> Hardening {
    if bytes.len() > 8192 {
        return Hardening::None;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Hardening::None;
    };
    let mut nnp = 0;
    let mut caps = 0;
    for line in text.split('\n') {
        if line.starts_with("NoNewPrivs:") {
            if line != "NoNewPrivs:\t1" {
                return Hardening::None;
            }
            nnp += 1;
        }
        if line.starts_with("CapBnd:") {
            if line != "CapBnd:\t00000000000000c1" {
                return Hardening::None;
            }
            caps += 1;
        }
    }
    if nnp == 1 && caps == 1 {
        Hardening::Standard
    } else {
        Hardening::None
    }
}
pub(crate) fn bin_alias(bytes: &[u8]) -> bool {
    bytes.len() <= 16 && (bytes == b"usr/bin" || bytes == b"/usr/bin")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sudo {
    Absent,
    Untrusted,
    ExitNonzero,
    ExitZero,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RightsFact {
    pub(crate) no_new_privs: Option<bool>,
    pub(crate) sudo: Sudo,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RightsLine {
    rights: RightsFact,
}
pub(crate) fn rights_line(text: &str) -> Option<RightsFact> {
    if text.len() > 96 {
        return None;
    }
    // Value enforces mandatory nullable key; typed decoding additionally rejects
    // duplicate keys at both levels (Value alone would silently overwrite them).
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value.get("rights")?.as_object()?.get("no_new_privs")?;
    Some(serde_json::from_str::<RightsLine>(text).ok()?.rights)
}
pub(crate) fn classify_rights(
    rights: Rights,
    hardening: Hardening,
    fact: RightsFact,
) -> Result<(), ExecutionUnavailable> {
    use ExecutionUnavailable::{
        MechanismUnavailable, RightsMismatch, ServiceReconfigurationRequired,
    };
    match (rights, hardening) {
        (Rights::Standard, Hardening::None) => Err(MechanismUnavailable),
        (Rights::Admin, Hardening::Standard) => Err(ServiceReconfigurationRequired),
        (Rights::Standard, Hardening::Standard) => match (fact.no_new_privs, fact.sudo) {
            (Some(true), Sudo::Absent | Sudo::ExitNonzero) => Ok(()),
            _ => Err(RightsMismatch),
        },
        (Rights::Admin, Hardening::None) => match (fact.no_new_privs, fact.sudo) {
            (Some(false), Sudo::ExitZero) => Ok(()),
            _ => Err(RightsMismatch),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn p1_location_and_single_parser() {
        for (input, components) in [
            ("0::/arxd.service/supervisor", vec!["arxd.service"]),
            (
                "0::/a.slice/b.slice/c.slice/arxd.service/supervisor\n",
                vec!["a.slice", "b.slice", "c.slice", "arxd.service"],
            ),
        ] {
            let l = locate(input.as_bytes()).unwrap();
            assert_eq!(l.unit_components(), components);
            let mut expected = PathBuf::from("/sys/fs/cgroup");
            for part in components {
                expected.push(part);
            }
            expected.push("runs");
            assert_eq!(l.runs(), expected);
        }
        for input in [
            "0::/a/supervisor",
            "0::/system.slice/foo.service/supervisor",
            "0::/arxd.service/x/supervisor",
            "0::/arxd.service.d/supervisor",
            "0::/arxd.service/supervisor\r\n",
            "0:://arxd.service/supervisor",
            "0::/./arxd.service/supervisor",
            "0::/../arxd.service/supervisor",
            "0::/arxd.service/supervisor\n1::/x",
            "0::/arxd.service/supervisor\0",
            "0::/arxd.service/supervisor\\",
        ] {
            assert!(locate(input.as_bytes()).is_none(), "{input:?}");
        }
        assert!(locate(&[255]).is_none());
        assert!(locate(&[b'a'; 4097]).is_none());
        let bridge = include_str!("../../service/unix_bridge.rs");
        assert!(!bridge.contains("fn cgroup_parent("));
        assert!(!bridge.contains("strip_prefix(\"0::"));
        assert!(bridge.contains("use crate::exec::hierarchy::locate;"));
        let bridge_tests = include_str!("../../service/unix_bridge/tests.rs");
        assert!(bridge_tests.contains(".map(|l| l.runs())"));
        assert_eq!(
            include_str!("parse.rs")
                .matches(concat!("pub(crate) fn ", "locate("))
                .count(),
            1
        );
    }
    #[test]
    fn p2_leaf_grammar() {
        for s in ["arx-0-0", "arx-4194304-18446744073709551615"] {
            assert!(leaf_name(s.as_bytes()));
        }
        for s in [
            "arx-01-2",
            "arx-1-02",
            "arx--1",
            "arx-1-",
            "arx-1-2-3",
            "ARX-1-2",
            "arx-+1-2",
            "arx-12345678901-1",
            "arx-1-123456789012345678901",
            "arx-1-2\n",
            "",
            "arx-1-2\0",
        ] {
            assert!(!leaf_name(s.as_bytes()), "{s}");
        }
    }
    #[test]
    fn p3_events_strict_keys() {
        for s in [
            "populated 0\nfrozen 0\n",
            "frozen 0\npopulated 0\n",
            "populated 0\nfuture_key 7\n",
            "populated 0",
        ] {
            assert_eq!(events(s), Some(false));
        }
        assert_eq!(events("populated 1\nfrozen 1\n"), Some(true));
        for s in [
            "populated 0\npopulated 1\n",
            "populated 0\nfrozen 0\nfrozen 0",
            "frozen 0",
            "populated 2",
            "Populated 0",
            "populated 0\nfrozen-x 0",
            "populated 0\nfrozen x",
            "populated 0\r\n",
            "populated 0\n\n",
            "populated  0",
            "populated 0\nfrozen +1",
            "",
        ] {
            assert_eq!(events(s), None, "{s:?}");
        }
        assert_eq!(events(&"a".repeat(4097)), None);
    }
    #[test]
    fn p4_controller_tokens() {
        for s in ["memoryx pids", "memory", "memory pidss", "xmemory pids", ""] {
            let c = controllers(s);
            assert!(!(c.memory && c.pids));
        }
        assert_eq!(
            controllers("cpu memory pids\n"),
            Controllers {
                memory: true,
                pids: true
            }
        );
        assert_eq!(
            controllers(&"memory pids ".repeat(400)),
            Controllers {
                memory: false,
                pids: false
            }
        );
    }
    #[test]
    fn p5_pid_decimals() {
        assert_eq!(pids(""), Some(vec![]));
        assert_eq!(pids("0\n1\n4294967295\n"), Some(vec![0, 1, u32::MAX]));
        assert_eq!(pids("42"), Some(vec![42]));
        for s in [
            "01",
            "+1",
            "1 2",
            "4294967296",
            "\n",
            "1\n\n",
            "1\r\n",
            "-1",
        ] {
            assert_eq!(pids(s), None);
        }
        assert_eq!(pids(&"1".repeat(4097)), None);
    }
    #[test]
    fn p6_exact_hardening() {
        let good = "NoNewPrivs:\t1\nCapBnd:\t00000000000000c1\n";
        assert_eq!(hardening(good.as_bytes()), Hardening::Standard);
        assert_eq!(
            hardening(format!("Name:\tarxd\n{good}").as_bytes()),
            Hardening::Standard
        );
        for s in [
            good.replace("00c1", "00c0"),
            good.replace("00c1", "01c1"),
            good.replace("00c1", "00C1"),
            good.replace("Privs:\t1", "Privs:\t0"),
            format!("{good}NoNewPrivs:\t1\n"),
            format!("{good}CapBnd:\t00000000000000c1\n"),
            "NoNewPrivs:\t1\n".into(),
            "CapBnd:\t00000000000000c1\n".into(),
            good.replace('\n', "\r\n"),
        ] {
            assert_eq!(hardening(s.as_bytes()), Hardening::None);
        }
        assert_eq!(hardening(&[b'a'; 8193]), Hardening::None);
        assert_eq!(hardening(&[255]), Hardening::None);
    }
    #[test]
    fn p7_strict_rights_json() {
        for (value, expected) in [
            ("absent", Sudo::Absent),
            ("untrusted", Sudo::Untrusted),
            ("exit_nonzero", Sudo::ExitNonzero),
            ("exit_zero", Sudo::ExitZero),
            ("unknown", Sudo::Unknown),
        ] {
            for nnp in ["true", "false", "null"] {
                let text = format!(r#"{{"rights":{{"no_new_privs":{nnp},"sudo":"{value}"}}}}"#);
                let parsed = rights_line(&text).unwrap();
                assert_eq!(parsed.sudo, expected);
                assert_eq!(
                    parsed.no_new_privs,
                    match nnp {
                        "true" => Some(true),
                        "false" => Some(false),
                        _ => None,
                    }
                );
            }
        }
        for s in [
            r#"{"rights":{"sudo":"absent"}}"#,
            r#"{"rights":{"no_new_privs":true}}"#,
            r#"{"rights":{"no_new_privs":"true","sudo":"absent"}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"yes"}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"effective"}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"refused"}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"absent","extra":0}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"absent"},"extra":0}"#,
            r#"{"rights":{"no_new_privs":true,"no_new_privs":false,"sudo":"absent"}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"absent","sudo":"absent"}}"#,
            r#"{"rights":{"no_new_privs":true,"sudo":"absent"},"rights":{"no_new_privs":true,"sudo":"absent"}}"#,
            r#"{"rights":null}"#,
            r#"{"rights":{"no_new_privs":0,"sudo":"absent"}}"#,
        ] {
            assert!(rights_line(s).is_none(), "{s}");
        }
        assert!(rights_line(&" ".repeat(97)).is_none());
    }
    #[test]
    fn p8_exhaustive_rights_table() {
        use ExecutionUnavailable::*;
        // Independent 2x2x3x5 literal table; sudo order is below, NNP true/false/null.
        let mismatch = Err(RightsMismatch);
        let table = [
            [
                [Ok(()), mismatch, Ok(()), mismatch, mismatch],
                [mismatch; 5],
                [mismatch; 5],
            ],
            [[Err(MechanismUnavailable); 5]; 3],
            [[Err(ServiceReconfigurationRequired); 5]; 3],
            [
                [mismatch; 5],
                [mismatch, mismatch, mismatch, Ok(()), mismatch],
                [mismatch; 5],
            ],
        ];
        for (row, (r, h)) in [
            (Rights::Standard, Hardening::Standard),
            (Rights::Standard, Hardening::None),
            (Rights::Admin, Hardening::Standard),
            (Rights::Admin, Hardening::None),
        ]
        .into_iter()
        .enumerate()
        {
            for (n, nnp) in [Some(true), Some(false), None].into_iter().enumerate() {
                for (s, sudo) in [
                    Sudo::Absent,
                    Sudo::Untrusted,
                    Sudo::ExitNonzero,
                    Sudo::ExitZero,
                    Sudo::Unknown,
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(
                        classify_rights(
                            r,
                            h,
                            RightsFact {
                                no_new_privs: nnp,
                                sudo
                            }
                        ),
                        table[row][n][s],
                        "row {row} nnp {n} sudo {s}"
                    );
                }
            }
        }
    }
    #[test]
    fn p9_portable_directory_types() {
        use Entry::*;
        for name in [b".".as_slice(), b".."] {
            assert_eq!(classify_entry(name, 4, None), Dot);
        }
        assert_eq!(classify_entry(b"arx-1-2", 4, None), Child);
        for name in ["cgroup.events", "memory.swap.max", "cgroup.pressure"] {
            assert_eq!(classify_entry(name.as_bytes(), 8, None), Control);
        }
        for name in [
            "arx-1-2",
            "Memory.max",
            ".hidden",
            "nodot",
            "a..b",
            "a.",
            "a/b.c",
            "",
        ] {
            assert_eq!(classify_entry(name.as_bytes(), 8, None), Refused);
        }
        assert_eq!(
            classify_entry(format!("{}.b", "a".repeat(63)).as_bytes(), 8, None),
            Refused
        );
        for dt in [10, 2, 6, 1, 12, 255] {
            for name in [b"arx-1-2".as_slice(), b"cgroup.events"] {
                assert_eq!(classify_entry(name, dt, Some(Ok(0o040700))), Refused);
            }
        }
        assert_eq!(classify_entry(b"arx-1-2", 0, Some(Ok(0o040700))), Child);
        assert_eq!(
            classify_entry(b"cgroup.events", 0, Some(Ok(0o100600))),
            Control
        );
        for mode in [
            None,
            Some(Err(())),
            Some(Ok(0o120777)),
            Some(Ok(0o020600)),
            Some(Ok(0)),
        ] {
            assert_eq!(classify_entry(b"cgroup.events", 0, mode), Refused);
        }
    }
    #[test]
    fn p10_exact_bin_alias() {
        for s in ["usr/bin", "/usr/bin"] {
            assert!(bin_alias(s.as_bytes()));
        }
        for s in [
            "usr/bin/",
            "./usr/bin",
            "usr//bin",
            "/usr/local/bin",
            "../usr/bin",
            "",
            "0123456789usr/bin",
            "xxxusr/bin",
            "usr/bin\0",
        ] {
            assert!(!bin_alias(s.as_bytes()));
        }
        assert!(!bin_alias(&[b'a'; 17]));
    }
    #[test]
    fn bounded_valid_inputs_and_total_byte_data() {
        let suffix = "/arxd.service/supervisor";
        let location = format!("0::/{}{suffix}", "a".repeat(4096 - 4 - suffix.len()));
        assert_eq!(location.len(), 4096);
        assert!(locate(location.as_bytes()).is_some());
        assert!(locate(format!("{location}\n").as_bytes()).is_none());
        let prefix = "populated 0\nfrozen ";
        let event = format!("{prefix}{}", "1".repeat(4096 - prefix.len()));
        assert_eq!(events(&event), Some(false));
        assert_eq!(events(&format!("{event}\n")), None);
        let ids = "1\n".repeat(2048);
        assert_eq!(pids(&ids).unwrap().len(), 2048);
        assert_eq!(pids(&format!("{ids}1")), None);
        let profile = "NoNewPrivs:\t1\nCapBnd:\t00000000000000c1\n";
        let profile = format!("{profile}{}", "x".repeat(8192 - profile.len()));
        assert_eq!(hardening(profile.as_bytes()), Hardening::Standard);
        assert_eq!(hardening(format!("{profile}x").as_bytes()), Hardening::None);
        let rights = r#"{"rights":{"no_new_privs":true,"sudo":"absent"}}"#;
        let rights = format!("{rights}{}", " ".repeat(96 - rights.len()));
        assert!(rights_line(&rights).is_some());
        assert!(rights_line(&format!("{rights} ")).is_none());
        assert_eq!(
            classify_entry(format!("{}.b", "a".repeat(62)).as_bytes(), 8, None),
            Entry::Control
        );
        // Every byte value, including invalid UTF-8, through bounded pure inputs.
        for byte in 0..=255 {
            let bytes = [byte; 128];
            let _ = locate(&bytes);
            let _ = leaf_name(&bytes);
            let _ = hardening(&bytes);
            let _ = bin_alias(&bytes);
            let _ = classify_entry(&bytes, byte, Some(Ok(u32::from(byte))));
            if let Ok(text) = std::str::from_utf8(&bytes) {
                let _ = events(text);
                let _ = controllers(text);
                let _ = pids(text);
                let _ = rights_line(text);
            }
        }
    }
}
