//! Private R5 observation. No service wiring or released metadata.
#[cfg(target_os = "linux")]
#[path = "unix_shim_rights_linux.rs"]
mod linux;

#[cfg(any(unix, test))]
pub(super) fn append(output: &mut Vec<u8>, fact: (Option<bool>, &'static str)) -> bool {
    // Literal key order is part of the private line protocol.
    let nnp = match fact.0 {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let line = format!(
        "{{\"rights\":{{\"no_new_privs\":{nnp},\"sudo\":\"{}\"}}}}\n",
        fact.1
    );
    if line.len() > 96 || output.len() + line.len() > 4096 {
        return false;
    }
    output.extend_from_slice(line.as_bytes());
    true
}

#[cfg(unix)]
pub(super) fn observe() -> (Option<bool>, &'static str) {
    #[cfg(target_os = "linux")]
    return (linux::nnp(), linux::probe());
    #[cfg(not(target_os = "linux"))]
    (None, "unknown")
}

#[cfg(test)]
mod tests {
    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn s11_non_linux_has_no_probe() {
        assert_eq!(super::observe(), (None, "unknown"));
    }

    #[test]
    fn s6_rights_last_and_bounded() {
        for nnp in [None, Some(false), Some(true)] {
            for sudo in [
                "absent",
                "untrusted",
                "exit_zero",
                "exit_nonzero",
                "unknown",
            ] {
                let mut bytes = b"tool\n".to_vec();
                assert!(super::append(&mut bytes, (nnp, sudo)));
                let text = std::str::from_utf8(&bytes).unwrap();
                let lines: Vec<_> = text.lines().collect();
                assert_eq!(lines.len(), 2);
                let value: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
                assert_eq!(value["rights"]["sudo"], sudo);
                assert_eq!(value["rights"]["no_new_privs"], serde_json::json!(nnp));
                let size = bytes.len() - 5;
                let mut exact = vec![b'x'; 4096 - size];
                assert!(super::append(&mut exact, (nnp, sudo)));
                assert_eq!(exact.len(), 4096);
                let mut excess = vec![b'x'; 4097 - size];
                assert!(!super::append(&mut excess, (nnp, sudo)));
            }
        }
    }

    #[test]
    fn s13_descriptor_execution_source_boundary() {
        let source = include_str!("unix_shim_rights_linux.rs");
        let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        for forbidden in [
            "Command::",
            "execvp(",
            "execv(",
            "posix_spawn",
            "var_os(",
            "var(",
            "getenv(",
        ] {
            assert!(!production.contains(forbidden), "fixed execution boundary");
        }
        assert!(production.contains("libc::execveat("));
        assert!(production.contains("libc::AT_EMPTY_PATH"));
    }
}
