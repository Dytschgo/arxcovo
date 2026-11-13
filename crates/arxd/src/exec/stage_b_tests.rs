use super::tests::{config, spec};
use super::*;

fn safe_entry() -> bool {
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        return false;
    }
    true
}
fn shim(tail: Vec<String>) -> RunSpec {
    let mut s = spec();
    s.arguments = vec!["exec-child".into(), "path".into(), "x".into(), "".into()];
    s.arguments.extend(tail);
    s
}
#[test]
fn r2_all_eight_and_private_failure_tags() {
    for cancel in [false, true] {
        for runner in [false, true] {
            for caller in [false, true] {
                let f = recheck(cancel, shutdown_requested(runner, caller));
                let expected = if runner || caller {
                    Some(NotStarted::Shutdown)
                } else if cancel {
                    Some(NotStarted::Cancelled)
                } else {
                    None
                };
                assert_eq!(
                    f.as_ref().map(|f| f.phase),
                    expected.map(RunError::NotStarted)
                );
                if let Some(f) = f {
                    assert_eq!(
                        f.legacy,
                        ExecError::Unavailable("run already cancelled or shutting down")
                    );
                }
            }
        }
    }
    for phase in [
        RunError::NotStarted(NotStarted::Resource),
        RunError::SpawnFailed,
        RunError::AfterSpawn,
    ] {
        let f = RunFailure::os(phase, std::io::Error::from_raw_os_error(7));
        assert_eq!(f.phase, phase);
        assert_eq!(f.legacy, ExecError::OperatingSystem(7));
        assert_eq!(
            RunFailure::os(phase, std::io::Error::other("synthetic")).legacy,
            ExecError::OperatingSystem(-1)
        );
    }
}
#[test]
fn shared_cancellation_and_runner_drop_are_independent() {
    let caller: crate::development::Cancellation = Cancellation::default();
    let runner = Runner::new(config()).unwrap();
    let own = runner.shutdown_handle();
    drop(runner);
    assert!(own.is_cancelled());
    assert!(!caller.is_cancelled());
    let shared: Cancellation = caller.clone();
    shared.cancel();
    assert!(caller.is_cancelled());
}
#[test]
fn l1b_exact_distinct_legacy_validation() {
    let runner = Runner::new(config()).unwrap();
    for (s, literal) in [
        (
            {
                let mut s = spec();
                s.cwd = "../bad".into();
                s
            },
            "absolute executable and workspace-relative cwd required",
        ),
        (
            {
                let mut s = spec();
                s.arguments = vec!["a".into(); 129];
                s
            },
            "argv bounds",
        ),
        (
            {
                let mut s = spec();
                s.timeout = Duration::ZERO;
                s
            },
            "timeout/output bounds",
        ),
    ] {
        assert_eq!(
            runner.run(&s, &Cancellation::default()).err(),
            Some(ExecError::Invalid(literal))
        );
        assert_eq!(
            runner
                .run_phased(&s, &Cancellation::default(), &Cancellation::default())
                .err(),
            Some(RunError::NotStarted(NotStarted::Invalid))
        );
    }
}
#[test]
fn l2_exact_prefix_and_tail_bounds() {
    for s in [shim(vec!["a".into(); 128]), shim(vec!["a".repeat(8192); 8])] {
        assert_eq!(validate_spec(&s), Err(ExecError::Invalid("argv bounds")));
        assert!(validate_phased_spec(&s).is_ok());
    }
    let mut tool = shim(vec!["a".into(); 128]);
    tool.arguments[1] = "tool".into();
    assert_eq!(validate_spec(&tool), Err(ExecError::Invalid("argv bounds")));
    assert!(validate_phased_spec(&tool).is_ok(), "tool prefix accepted");
    let mut wide = shim(vec!["a".repeat(512); 128]);
    wide.arguments[2] = "a".repeat(4096);
    wide.arguments[3] = "b".repeat(4096);
    assert!(validate_phased_spec(&wide).is_ok());
    wide.arguments[1] = "tool".into();
    assert!(
        validate_phased_spec(&wide).is_ok(),
        "wide tool prefix accepted"
    );
    let mut cases = vec![
        shim(vec!["a".into(); 129]),
        shim(vec!["a".repeat(8192); 9]),
        shim(vec!["a".repeat(8193)]),
    ];
    let mut over = shim(vec!["a".repeat(8192); 8]);
    over.arguments.push("b".into());
    cases.push(over);
    for (index, value) in [
        (1, "other".into()),
        (2, "a".repeat(4097)),
        (3, "a".repeat(4097)),
        (2, "".into()),
    ] {
        let mut s = shim(vec![]);
        s.arguments[index] = value;
        cases.push(s);
    }
    for index in 0..4 {
        let mut s = shim(vec![]);
        s.arguments[index].push('\0');
        cases.push(s);
    }
    for len in 1..4 {
        let mut s = shim(vec![]);
        s.arguments.truncate(len);
        cases.push(s);
    }
    let mut s = spec();
    s.arguments = vec!["a".into(); 132];
    cases.push(s);
    for s in cases {
        assert!(validate_phased_spec(&s).is_err(), "phased boundary refused");
    }
    assert!(validate_phased_spec(&spec()).is_ok());
}
#[test]
fn l2b_actual_legacy_entry_refuses_both_bounds() {
    if !safe_entry() {
        return;
    }
    let runner = Runner::new(config()).unwrap();
    for s in [shim(vec!["a".into(); 128]), shim(vec!["a".repeat(8192); 8])] {
        assert_eq!(
            runner.run(&s, &Cancellation::default()).err(),
            Some(ExecError::Invalid("argv bounds"))
        );
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn r1_l1_marker_never_executes() {
    if !safe_entry() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("arx-stage-b-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let marker = dir.join("marker");
    let executable = dir.join("executable");
    // Owned synthetic target only, never a build account or resolver.
    std::fs::write(&executable, b"#!/bin/sh\nprintf executed > \"$1\"\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut s = spec();
    s.executable = executable;
    s.arguments = vec![marker.to_str().unwrap().into()];
    let runner = Runner::new(config()).unwrap();
    assert_eq!(
        runner.run(&s, &Cancellation::default()).err(),
        Some(ExecError::Unavailable(
            "root service identity required; no owner fallback"
        ))
    );
    assert_eq!(
        runner
            .run_phased(&s, &Cancellation::default(), &Cancellation::default())
            .err(),
        Some(RunError::NotStarted(NotStarted::ServiceIdentity))
    );
    assert!(!marker.exists(), "marker never executed");
    std::fs::remove_file(s.executable).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn l3_exact_unsupported() {
    let runner = Runner::new(config()).unwrap();
    assert_eq!(
        runner.run(&spec(), &Cancellation::default()).err(),
        Some(ExecError::Unavailable(
            "execution backend unavailable on this OS"
        ))
    );
    assert_eq!(
        runner
            .run_phased(&spec(), &Cancellation::default(), &Cancellation::default())
            .err(),
        Some(RunError::NotStarted(NotStarted::Unsupported))
    );
}

// Lexer preserves literals but strips nested comments. Test-module exclusion is
// structural and requires the inline module to be the final item.
fn tokens(source: &str) -> Vec<String> {
    let b = source.as_bytes();
    let (mut i, mut out) = (0, Vec::new());
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while depth > 0 {
                assert!(i < b.len(), "closed comment");
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let start = i;
        let raw = if b[i..].starts_with(b"br") {
            i + 2
        } else if b[i] == b'r' {
            i + 1
        } else {
            i
        };
        let mut quote = raw;
        while b.get(quote) == Some(&b'#') {
            quote += 1;
        }
        if raw != i && b.get(quote) == Some(&b'"') {
            let hashes = quote - raw;
            i = quote + 1;
            loop {
                assert!(i < b.len(), "closed raw string");
                if b[i] == b'"'
                    && b.get(i + 1..i + 1 + hashes)
                        .is_some_and(|h| h.iter().all(|c| *c == b'#'))
                {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if b[i] == 39
            && (source[i + 1..]
                .chars()
                .next()
                .is_some_and(|c| b.get(i + 1 + c.len_utf8()) == Some(&39))
                || b.get(i + 1) == Some(&92))
        {
            i += 1;
            while i < b.len() {
                if b[i] == 92 {
                    i += 2;
                } else if b[i] == 39 {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
        out.push(source[start..i].to_owned());
    }
    out
}
fn scan(source: &str) -> Result<Vec<(String, String)>, &'static str> {
    scan_with_hook(source, false)
}
fn scan_with_hook(source: &str, shim: bool) -> Result<Vec<(String, String)>, &'static str> {
    let mut t = tokens(source);
    let boundary = tokens("#[cfg(test)] mod tests {");
    let hits: Vec<_> = t
        .windows(boundary.len())
        .enumerate()
        .filter(|(_, w)| *w == boundary)
        .map(|(i, _)| i)
        .collect();
    if hits.len() > 1 {
        return Err("multiple test modules");
    }
    if let Some(&start) = hits.first() {
        let mut depth = 1;
        let mut end = start + boundary.len();
        while depth > 0 && end < t.len() {
            if t[end] == "{" {
                depth += 1;
            }
            if t[end] == "}" {
                depth -= 1;
            }
            end += 1;
        }
        if depth != 0 || end != t.len() {
            return Err("test module must be trailing");
        }
        t.truncate(start);
    }
    let hooks = t.iter().filter(|v| *v == "set_hook").count();
    if hooks != usize::from(shim) {
        return Err("hook inventory");
    }
    if !shim
        && (t.windows(3).any(|v| v == [":", ":", "stderr"])
            || t.windows(2).any(|v| v == ["stderr", "as"]))
    {
        return Err("stderr writer");
    }
    let mut inventory = Vec::new();
    for (i, token) in t.iter().enumerate() {
        if ["panic_any", "resume_unwind", "take_hook", "macro_rules"].contains(&token.as_str()) {
            return Err("forbidden diagnostic");
        }
        if ["expect", "unwrap", "unwrap_err", "expect_err"].contains(&token.as_str()) {
            return Err("fallible payload");
        }
        if token == "use" {
            let end = t[i..]
                .iter()
                .position(|v| v == ";")
                .map_or(t.len(), |n| i + n);
            if t[i..end].iter().any(|v| {
                [
                    "RunFailure",
                    "tracing",
                    "log",
                    "set_hook",
                    "take_hook",
                    "stderr",
                ]
                .contains(&v.as_str())
            }) {
                return Err("diagnostic import alias");
            }
        }
        if token == "type" {
            let end = t[i..]
                .iter()
                .position(|v| v == ";")
                .map_or(t.len(), |n| i + n);
            if t[i..end].iter().any(|v| v == "RunFailure") {
                return Err("failure alias");
            }
        }
        if ["dbg", "print", "println", "eprint", "eprintln", "writeln"].contains(&token.as_str()) {
            return Err("diagnostic name");
        }
        if token == "impl" {
            let end = t[i..]
                .iter()
                .position(|v| v == "{")
                .map_or(t.len(), |n| i + n);
            let header = &t[i..end];
            if header.iter().any(|v| v == "From") && header.iter().any(|v| v == "RunFailure") {
                return Err("implicit failure conversion");
            }
        }
        if t.get(i + 1).is_none_or(|v| v != "!") {
            continue;
        }
        if [
            "assert_eq",
            "assert_ne",
            "debug_assert_eq",
            "debug_assert_ne",
            "dbg",
            "print",
            "println",
            "eprint",
            "eprintln",
        ]
        .contains(&token.as_str())
        {
            return Err("formatted diagnostic");
        }
        if [
            "panic",
            "unreachable",
            "assert",
            "debug_assert",
            "todo",
            "unimplemented",
        ]
        .contains(&token.as_str())
        {
            if t.get(i + 2).is_none_or(|v| v != "(") {
                return Err("panic delimiter");
            }
            let mut stack = vec![")"];
            let mut end = i + 3;
            let mut commas = Vec::new();
            while end < t.len() && !stack.is_empty() {
                let v = t[end].as_str();
                match v {
                    "(" => stack.push(")"),
                    "[" => stack.push("]"),
                    "{" => stack.push("}"),
                    ")" | "]" | "}" if stack.pop() != Some(v) => {
                        return Err("unbalanced macro");
                    }
                    "," if stack.len() == 1 => commas.push(end),
                    _ => (),
                }
                if !stack.is_empty() {
                    end += 1;
                }
            }
            if !stack.is_empty() {
                return Err("unclosed macro");
            }
            let args = &t[i + 3..end];
            let assertion = token == "assert" || token == "debug_assert";
            let msg = if assertion {
                if commas.len() != 1 || commas[0] == i + 3 || commas[0] + 2 != end {
                    return Err("assert message required");
                }
                &t[commas[0] + 1]
            } else {
                if args.len() != 1 {
                    return Err("one literal required");
                }
                &args[0]
            };
            if !msg.starts_with('"') || !msg.ends_with('"') || msg.contains(['{', '}', '\\']) {
                return Err("fixed literal required");
            }
            inventory.push((token.clone(), msg.clone()));
        }
    }
    Ok(inventory)
}
fn inventory() -> Vec<(&'static str, String, String)> {
    let mut all = Vec::new();
    for (file, source) in [
        ("mod", include_str!("mod.rs")),
        ("unix", include_str!("unix.rs")),
        ("cgroup", include_str!("cgroup.rs")),
        ("output", include_str!("output.rs")),
        ("shim", include_str!("../unix_shim.rs")),
        ("shim-rights", include_str!("../unix_shim_rights.rs")),
        (
            "shim-rights-linux",
            include_str!("../unix_shim_rights_linux.rs"),
        ),
        ("bridge", include_str!("../service/unix_bridge.rs")),
    ] {
        for (mac, literal) in
            scan_with_hook(source, file == "shim").expect("production payload scan")
        {
            all.push((file, mac, literal));
        }
    }
    all.sort();
    all
}
#[test]
fn p_scan_checked_nonempty_inventory() {
    let expected = vec![
        ("bridge", "panic".into(), "\"ARX-T10-MAPPING-PANIC\"".into()),
        (
            "bridge",
            "panic".into(),
            "\"ARX-T10-DISCOVERY-PANIC\"".into(),
        ),
        ("cgroup", "unreachable".into(), "\"numeric name\"".into()),
        ("unix", "unreachable".into(), "\"validated cwd\"".into()),
        ("unix", "unreachable".into(), "\"validated path\"".into()),
        ("unix", "unreachable".into(), "\"validated path\"".into()),
    ];
    assert!(!expected.is_empty());
    let mut expected = expected;
    expected.sort();
    assert_eq!(inventory(), expected);
}
#[test]
fn p_scan_negative_controls_and_no_blind_test_skip() {
    for source in [
        r#"panic!("{x:?}");"#,
        ".expect(\"marker\")",
        ".unwrap()",
        ".unwrap_err()",
        ".expect_err(\"x\")",
        "assert_eq!(a,b)",
        "debug_assert_ne!(a,b)",
        "dbg!(x)",
        "panic_any(x)",
        "resume_unwind(x)",
        "println!(x)",
        "set_hook(x)",
        "take_hook()",
        "Result::unwrap(r)",
        "use std::io::stderr as stream; stream().write_all(x)",
        "macro_rules! hidden { () => { r.unwrap() } }",
        "use std::panic::set_hook as install; install(x)",
        ".map(Option::unwrap)",
        "r.r#unwrap()",
        "use std::eprintln as say; say!(x)",
        "type F = RunFailure; impl From<E> for F {}",
        "use super::RunFailure as F; impl From<E> for F {}",
        "use tracing::error as report; report!(x)",
        "writeln!(std::io::stderr(), x)",
        "impl From<E> for RunFailure {}",
        "#[cfg(test)] mod tests {} fn production() {x.unwrap();}",
    ] {
        assert!(scan(source).is_err(), "synthetic diagnostic rejected");
    }
    assert!(
        scan("/* .expect( */ fn f() { let s = \".unwrap()\"; }")
            .unwrap()
            .is_empty()
    );
    assert_eq!(scan("unreachable!(\"fixed\");").unwrap().len(), 1);
    assert_eq!(scan("assert!(f(a, b), \"fixed\");").unwrap().len(), 1);
}

#[test]
fn p_scan_hook_site_is_exactly_one_and_shim_only() {
    assert!(scan_with_hook("fn f() {}", true).is_err());
    assert!(scan_with_hook("set_hook(x); set_hook(y);", true).is_err());
    assert!(scan_with_hook("set_hook(x);", true).is_ok());
    assert!(scan_with_hook("set_hook(x);", false).is_err());
}

// E3 extends the existing tokenizer and payload scanner on every host. Native
// syscall tests remain Linux-only; this test cannot establish Linux behavior.
fn e3_source_allowed(source: &str) -> bool {
    if scan(source).is_err() {
        return false;
    }
    let mut t = tokens(source);
    let boundary = tokens("#[cfg(test)] mod tests {");
    if let Some(i) = t.windows(boundary.len()).position(|w| w == boundary) {
        t.truncate(i);
    }
    let calls = [
        "read_dir",
        "canonicalize",
        "metadata",
        "exists",
        "kill",
        "killpg",
        "setpriority",
        "dup",
        "dup2",
    ];
    !t.iter()
        .any(|s| s == "Command" || s == "F_DUPFD" || s == "F_DUPFD_CLOEXEC")
        && !t
            .windows(2)
            .any(|w| calls.contains(&w[0].as_str()) && w[1] == "(")
        && !t.windows(5).any(|w| w == ["File", ":", ":", "open", "("])
}
#[test]
fn l6_e3_tokenized_source_inventory_and_mutants() {
    let sources = [
        ("hierarchy.rs", include_str!("hierarchy.rs")),
        ("hierarchy/linux.rs", include_str!("hierarchy/linux.rs")),
    ];
    assert!(
        sources
            .iter()
            .all(|(name, source)| *name != "hierarchy/fake.rs" && e3_source_allowed(source))
    );
    for mutant in [
        "read_dir(p)",
        "File::open(p)",
        "canonicalize(p)",
        "metadata(p)",
        "exists(p)",
        "kill(p)",
        "killpg(p)",
        "setpriority(p)",
        "Command::new(p)",
        "set_hook(p)",
        "dup(p)",
        "dup2(p)",
        "fcntl(p,F_DUPFD)",
        "fcntl(p,F_DUPFD_CLOEXEC)",
        "panic!(\"{p}\")",
    ] {
        assert!(!e3_source_allowed(mutant), "E3 source mutant rejected");
    }
    assert!(e3_source_allowed("// dup(p)\nlet s = \"kill(p)\";"));
}

// E4 extends the existing tokenizer/payload policy without a hook exemption.
fn e4_source_allowed(source: &str) -> bool {
    if scan(source).is_err() {
        return false;
    }
    let mut t = tokens(source);
    let boundary = tokens("#[cfg(test)] mod tests {");
    if let Some(i) = t.windows(boundary.len()).position(|w| w == boundary) {
        t.truncate(i);
    }
    !t.iter().any(|v| {
        [
            "Command",
            "execv",
            "execvp",
            "execve",
            "fexecve",
            "posix_spawn",
            "posix_spawnp",
            "SYS_execve",
            "SYS_execveat",
            "syscall",
            "getenv",
            "var",
            "var_os",
            "vars",
            "vars_os",
        ]
        .contains(&v.as_str())
    }) && !t
        .windows(2)
        .any(|w| (w[0] == "env" || w[0] == "option_env") && w[1] == "!")
}
#[test]
fn s13_e4_tokenized_source_and_bypass_controls() {
    assert!(
        e4_source_allowed(include_str!("../unix_shim_rights_linux.rs")),
        "E4 fixed execution boundary"
    );
    for mutant in [
        "Command::new(p)",
        "std::process::Command::<T>::new(p)",
        "execve(p,a,e)",
        "libc::syscall(libc::SYS_execve,p)",
        "use std::env::var_os as lookup;",
        "env!(\"PATH\")",
        "panic!(\"{p}\")",
        "set_hook(p)",
        "r.r#unwrap()",
        "use std::eprintln as say;",
    ] {
        assert!(!e4_source_allowed(mutant), "E4 bypass rejected");
    }
}

#[test]
fn s12_e4_portable_production_pins() {
    let t = tokens(include_str!("../unix_shim_rights_linux.rs"));
    for snippet in [
        "const ANCHOR: &CStr = c\"/\";",
        "const OWNER: u32 = 0;",
        "const CANDIDATES: &[&[&CStr]] = &[ &[c\"usr\", c\"bin\", c\"sudo\"], &[c\"usr\", c\"local\", c\"bin\", c\"sudo\"], &[c\"bin\", c\"sudo\"], ];",
        "const ALIASES: &[&[u8]] = &[b\"usr/bin\", b\"/usr/bin\"];",
        "const ARGV: &[&CStr] = &[c\"sudo\", c\"-n\", c\"true\"];",
        "const ENV: &[&CStr] = &[];",
        "const DEADLINE: Duration = Duration::from_secs(5);",
        "const NULL: &CStr = c\"/dev/null\";",
        "const CWD: &CStr = c\"/\";",
        "#[cfg(not(test))] ANCHOR",
        "#[cfg(not(test))] OWNER",
        "#[cfg(not(test))] DEADLINE",
        "#[cfg(not(test))] let mut values: Vec<_> = ARGV.iter().map(|s| s.as_ptr()).collect();",
        "#[cfg(not(test))] let mut values: Vec<_> = ENV.iter().map(|s| s.as_ptr()).collect();",
        "#[cfg(test)] hook: impl FnOnce()",
        "#[cfg(test)] hook();",
        "libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC",
    ] {
        let pin = tokens(snippet);
        assert!(t.windows(pin.len()).any(|w| w == pin), "E4 production pin");
    }
    let shim = tokens(include_str!("../unix_shim.rs"));
    let tools = shim.iter().position(|v| v == "TOOLS").unwrap();
    let append = tokens("rights::append(");
    let line = shim
        .windows(append.len())
        .position(|w| w == append)
        .unwrap();
    assert!(tools < line, "rights follow tools");
}
