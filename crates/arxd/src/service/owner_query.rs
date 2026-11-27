//! Bounded, output-free manager observations for ADR 0014.
//! The launchd absence tuple is pinned from hosted native evidence; the loaded
//! profile is synthetic until an installed capture exists (ADR 0014 addendum).
use super::layout::{Error, Platform};
pub const DROPIN_PATH: &str = "/etc/systemd/system/arxd.service.d/50-arxcovo-standard-rights.conf";
pub const SYSTEMD_QUERY: &[&str] = &[
    "show",
    "--property=LoadState,ActiveState,FragmentPath,DropInPaths",
    "arxd.service",
];
pub const UNIT_PATH: &str = "/etc/systemd/system/arxd.service";
pub const PLIST_PATH: &str = "/Library/LaunchDaemons/com.arxcovo.arxd.plist";
pub const LAUNCHD_TARGET: &str = "system/com.arxcovo.arxd";
pub const LAUNCHD_QUERY: &[&str] = &["print", LAUNCHD_TARGET];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Reloading,
    Starting,
    Stopping,
    Stopped,
    Failed,
    Loaded,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    Absent,
    Ours(State),
    Foreign,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    Installable,
    NotForeign,
    Installed,
    Unloaded,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Artifact {
    Absent,
    Identical,
    Foreign,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Always,
    Loaded,
    NotLoaded,
    ArtifactPresent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    Running,
    Starting,
    Stopping,
    Stopped,
    Failed,
}
impl Status {
    pub fn token(self) -> &'static str {
        match self {
            Self::NotInstalled => "not-installed",
            Self::Running => "running",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}
impl Query {
    fn safe(self, artifact: Artifact) -> Result<(), Error> {
        if artifact == Artifact::Foreign || self == Self::Foreign {
            return Err(Error::Foreign);
        }
        if self == Self::Unknown {
            return Err(Error::Manager);
        }
        Ok(())
    }
    fn active(self, platform: Platform) -> bool {
        if platform == Platform::Macos {
            return matches!(self, Self::Ours(_));
        }
        matches!(
            self,
            Self::Ours(
                State::Running
                    | State::Reloading
                    | State::Starting
                    | State::Stopping
                    | State::Loaded
            )
        )
    }
    pub fn expect(
        self,
        expectation: Expect,
        primary: Artifact,
        platform: Platform,
    ) -> Result<(), Error> {
        self.safe(primary)?;
        if platform == Platform::Windows {
            return Err(Error::Layout);
        }
        match expectation {
            Expect::Installable if self.active(platform) => Err(Error::Active),
            Expect::Installed
                if primary != Artifact::Identical
                    || (platform == Platform::Linux && self == Self::Absent) =>
            {
                Err(Error::NotInstalled)
            }
            Expect::Unloaded if self.active(platform) => Err(Error::Manager),
            _ => Ok(()),
        }
    }
    pub fn gate(self, gate: Gate, primary: Artifact) -> Result<bool, Error> {
        self.safe(primary)?;
        Ok(match gate {
            Gate::Always => true,
            Gate::Loaded => matches!(self, Self::Ours(_)),
            Gate::NotLoaded => self == Self::Absent,
            Gate::ArtifactPresent => primary == Artifact::Identical,
        })
    }
    pub fn macos_status(self, primary: Artifact) -> Result<Status, Error> {
        match (primary, self) {
            (Artifact::Absent, Self::Absent) => Ok(Status::NotInstalled),
            (Artifact::Identical, Self::Absent) => Ok(Status::Stopped),
            (Artifact::Identical, Self::Ours(State::Running)) => Ok(Status::Running),
            (Artifact::Identical, Self::Ours(State::Starting)) => Ok(Status::Starting),
            // Loaded but idle after a clean exit. `start` skips bootstrap for a
            // loaded job, so recovery is `stop` (bootout) followed by `start`.
            (Artifact::Identical, Self::Ours(State::Stopped)) => Ok(Status::Stopped),
            _ => Err(Error::Manager),
        }
    }
    pub fn linux_status(self, primary: Artifact) -> Result<Status, Error> {
        self.safe(primary)?;
        match (primary, self) {
            (Artifact::Absent, Self::Absent) => Ok(Status::NotInstalled),
            (Artifact::Identical, Self::Ours(state)) => match state {
                State::Running | State::Reloading => Ok(Status::Running),
                State::Starting => Ok(Status::Starting),
                State::Stopping => Ok(Status::Stopping),
                State::Stopped => Ok(Status::Stopped),
                State::Failed => Ok(Status::Failed),
                State::Loaded => Err(Error::Manager),
            },
            _ => Err(Error::Manager),
        }
    }
}
/// Every property occurs exactly once. Raw bytes never escape this parser.
pub fn systemd(success: bool, stdout: &[u8]) -> Query {
    if !success || stdout.len() > 4096 {
        return Query::Unknown;
    }
    let Ok(text) = std::str::from_utf8(stdout) else {
        return Query::Unknown;
    };
    let mut fields = [None; 4];
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return Query::Unknown;
        };
        let index = match key {
            "LoadState" => 0,
            "ActiveState" => 1,
            "FragmentPath" => 2,
            "DropInPaths" => 3,
            _ => return Query::Unknown,
        };
        if fields[index].replace(value).is_some() {
            return Query::Unknown;
        }
    }
    let [Some(load), Some(active), Some(fragment), Some(dropin)] = fields else {
        return Query::Unknown;
    };
    let state = match active {
        "active" => State::Running,
        "reloading" => State::Reloading,
        "activating" => State::Starting,
        "deactivating" => State::Stopping,
        "inactive" => State::Stopped,
        "failed" => State::Failed,
        _ => return Query::Unknown,
    };
    if load == "masked" {
        return Query::Foreign;
    }
    if !matches!(load, "loaded" | "not-found") {
        return Query::Unknown;
    }
    if (!fragment.is_empty() && fragment != UNIT_PATH)
        || (!dropin.is_empty() && dropin != DROPIN_PATH)
    {
        return Query::Foreign;
    }
    match load {
        "not-found" if fragment.is_empty() && dropin.is_empty() && state == State::Stopped => {
            Query::Absent
        }
        "loaded" if fragment == UNIT_PATH => Query::Ours(state),
        _ => Query::Unknown,
    }
}
/// Fixed launchd label of the arxd system daemon.
pub const LAUNCHD_LABEL: &str = "com.arxcovo.arxd";
/// `launchctl print` exit status for a label missing from the system domain.
/// Pinned together with the complete stream tuple, never on its own (ADR 0014
/// launchd profile addendum, 2026-11-27).
pub const LAUNCHD_ABSENT_EXIT: i32 = 113;
const LAUNCHD_PROGRAM: &str = "/Library/Application Support/Arxcovo/libexec/arxd";
const LAUNCHD_ARGUMENTS: &[&str] = &[LAUNCHD_PROGRAM, "service", "run"];
const LAUNCHD_STDOUT_CAP: usize = 65_536;
const LAUNCHD_STDERR_CAP: usize = 4_096;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LaunchdTermination {
    Exited(i32),
    Signalled,
    Incomplete,
}
/// Borrowed capture boundary. Deliberately no Debug/Display of private streams.
pub struct LaunchdObservation<'a> {
    pub termination: LaunchdTermination,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub stdout_complete: bool,
    pub stderr_complete: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}
/// Identity a loaded registration must show to count as ours: system domain
/// and label (header), plist path, program and the complete argument vector.
struct LaunchdProfile<'a> {
    label: &'a str,
    plist: &'a str,
    program: &'a str,
    arguments: &'a [&'a str],
}
const ARXD_PROFILE: LaunchdProfile<'static> = LaunchdProfile {
    label: LAUNCHD_LABEL,
    plist: PLIST_PATH,
    program: LAUNCHD_PROGRAM,
    arguments: LAUNCHD_ARGUMENTS,
};
/// Bounded, fail-closed reading of `launchctl print system/com.arxcovo.arxd`.
/// Raw bytes never escape this parser. launchctl(1) calls `print` output
/// unstable, so anything outside the reviewed tuples is Unknown.
pub fn launchd(observation: LaunchdObservation<'_>) -> Query {
    launchd_with(&observation, &ARXD_PROFILE)
}
/// Test-only absence check of a deliberately unregistered probe label.
#[cfg(test)]
pub(super) fn launchd_probe(observation: &LaunchdObservation<'_>, label: &str) -> Query {
    launchd_with(
        observation,
        &LaunchdProfile {
            label,
            ..ARXD_PROFILE
        },
    )
}
fn launchd_with(o: &LaunchdObservation<'_>, profile: &LaunchdProfile<'_>) -> Query {
    if !(o.stdout_complete && o.stderr_complete)
        || o.stdout_truncated
        || o.stderr_truncated
        || o.stdout.len() > LAUNCHD_STDOUT_CAP
        || o.stderr.len() > LAUNCHD_STDERR_CAP
    {
        return Query::Unknown;
    }
    let (Ok(stdout), Ok(stderr)) = (std::str::from_utf8(o.stdout), std::str::from_utf8(o.stderr))
    else {
        return Query::Unknown;
    };
    match o.termination {
        LaunchdTermination::Exited(LAUNCHD_ABSENT_EXIT)
            if stdout.is_empty() && stderr == absent_stderr(profile.label) =>
        {
            Query::Absent
        }
        LaunchdTermination::Exited(0) if stderr.is_empty() => loaded(stdout, profile),
        _ => Query::Unknown,
    }
}
/// The complete native stderr for a label missing from the system domain.
fn absent_stderr(label: &str) -> String {
    format!("Bad request.\nCould not find service \"{label}\" in domain for system\n")
}
/// Top-level fields of exactly one `system/<label> = { ... }` block. Nested
/// blocks must balance by tab depth and are ignored, except the top-level
/// argument vector, whose entries are compared in order.
///
/// Grammar: the only permitted blank lines are completely empty lines between
/// top-level entries. A whitespace-only line, a blank line inside any nested
/// block (including the argument vector) and anything after the closing `}`
/// other than one final newline are Unknown. `path`, `state` and `program`
/// must be scalars and `arguments` a block, each exactly once.
fn loaded(text: &str, profile: &LaunchdProfile<'_>) -> Query {
    if text.contains(['\r', '\0']) {
        return Query::Unknown;
    }
    let mut lines = text.strip_suffix('\n').unwrap_or(text).split('\n');
    if lines.next() != Some(format!("system/{} = {{", profile.label).as_str()) {
        return Query::Unknown;
    }
    let (mut path, mut state, mut program) = (None, None, None);
    let mut arguments: Option<Vec<&str>> = None;
    let mut seen = [false; 4];
    let mut collecting = false;
    let mut depth = 1usize;
    for line in lines {
        if depth == 0 {
            return Query::Unknown;
        }
        if line.is_empty() {
            if depth == 1 {
                continue;
            }
            return Query::Unknown;
        }
        if line.trim().is_empty() {
            return Query::Unknown;
        }
        let tabs = line.bytes().take_while(|b| *b == b'\t').count();
        let body = &line[tabs..];
        if body.starts_with(' ') {
            return Query::Unknown;
        }
        if body == "}" {
            if tabs + 1 != depth {
                return Query::Unknown;
            }
            depth -= 1;
            if depth == 1 {
                collecting = false;
            }
            continue;
        }
        if tabs != depth {
            return Query::Unknown;
        }
        if depth > 1 {
            if collecting {
                // The argument vector is flat; a nested block there is ambiguous.
                if body.ends_with('{') {
                    return Query::Unknown;
                }
                if let Some(list) = arguments.as_mut() {
                    list.push(body);
                }
            } else if body.ends_with('{') {
                depth += 1;
            }
            continue;
        }
        let Some((key, value)) = body.split_once(" = ") else {
            return Query::Unknown;
        };
        let block = value == "{";
        // Uniqueness and shape of required keys are settled before dispatch,
        // across both scalar and block entries.
        let required = ["path", "state", "program", "arguments"]
            .iter()
            .position(|k| *k == key);
        if let Some(index) = required
            && (std::mem::replace(&mut seen[index], true) || block != (key == "arguments"))
        {
            return Query::Unknown;
        }
        match key {
            "path" => path = Some(value),
            "state" => state = Some(value),
            "program" => program = Some(value),
            "arguments" => {
                arguments = Some(Vec::new());
                collecting = true;
                depth += 1;
            }
            _ if block => depth += 1,
            _ => {}
        }
    }
    if depth != 0 {
        return Query::Unknown;
    }
    let (Some(path), Some(state), Some(program), Some(arguments)) =
        (path, state, program, arguments)
    else {
        return Query::Unknown;
    };
    // An unrecognized state is ambiguous output, never proof of a foreign job.
    let state = match state {
        "running" => State::Running,
        "spawn scheduled" | "starting" => State::Starting,
        "not running" | "waiting" => State::Stopped,
        _ => return Query::Unknown,
    };
    if path != profile.plist || program != profile.program || arguments != profile.arguments {
        return Query::Foreign;
    }
    Query::Ours(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(load: &str, state: &str, path: &str, dropin: &str) -> Vec<u8> {
        format!(
            "LoadState={load}\nActiveState={state}\nFragmentPath={path}\nDropInPaths={dropin}\n"
        )
        .into_bytes()
    }
    #[test]
    fn bounded_queries_reject_ambiguous_or_foreign_output() {
        let unit = UNIT_PATH;
        let good = output("loaded", "inactive", unit, DROPIN_PATH);
        assert_eq!(systemd(true, &good), Query::Ours(State::Stopped));
        assert_eq!(systemd(false, &good), Query::Unknown);
        for field in [
            "LoadState=loaded\n",
            "ActiveState=inactive\n",
            "FragmentPath=\n",
            "DropInPaths=\n",
        ] {
            let mut duplicate = good.clone();
            duplicate.extend_from_slice(field.as_bytes());
            assert_eq!(systemd(true, &duplicate), Query::Unknown);
        }
        for bytes in [vec![b'x'; 4097], vec![255], b"LoadState=loaded\n".to_vec()] {
            assert_eq!(systemd(true, &bytes), Query::Unknown);
        }
        for (load, state, path, dropin, want) in [
            ("not-found", "inactive", "", "", Query::Absent),
            ("loaded", "active", unit, "", Query::Ours(State::Running)),
            (
                "loaded",
                "reloading",
                unit,
                "",
                Query::Ours(State::Reloading),
            ),
            (
                "loaded",
                "activating",
                unit,
                "",
                Query::Ours(State::Starting),
            ),
            (
                "loaded",
                "deactivating",
                unit,
                "",
                Query::Ours(State::Stopping),
            ),
            ("loaded", "failed", unit, "", Query::Ours(State::Failed)),
            ("masked", "inactive", "", "", Query::Foreign),
            ("loaded", "inactive", "/foreign", "", Query::Foreign),
            ("loaded", "inactive", unit, "/foreign", Query::Foreign),
            ("loaded", "other", unit, "", Query::Unknown),
            ("other", "inactive", unit, "", Query::Unknown),
        ] {
            assert_eq!(systemd(true, &output(load, state, path, dropin)), want);
        }
    }

    #[test]
    fn not_found_cannot_hide_an_active_or_transitioning_unit() {
        for active in [
            "active",
            "reloading",
            "activating",
            "deactivating",
            "failed",
        ] {
            assert_eq!(
                systemd(true, &output("not-found", active, "", "")),
                Query::Unknown
            );
        }
        assert_eq!(
            systemd(true, &output("not-found", "inactive", "", "")),
            Query::Absent
        );
    }

    #[test]
    fn active_unknown_and_foreign_cannot_pass_installable_or_stop_proof() {
        for state in [
            State::Running,
            State::Reloading,
            State::Starting,
            State::Stopping,
            State::Loaded,
        ] {
            let query = Query::Ours(state);
            assert_eq!(
                query.expect(Expect::Installable, Artifact::Absent, Platform::Linux),
                Err(Error::Active)
            );
            assert_eq!(
                query.expect(Expect::Unloaded, Artifact::Identical, Platform::Linux),
                Err(Error::Manager)
            );
        }
        for query in [
            Query::Absent,
            Query::Ours(State::Stopped),
            Query::Ours(State::Failed),
        ] {
            assert_eq!(
                query.expect(Expect::Installable, Artifact::Identical, Platform::Linux),
                Ok(())
            );
        }
        for expectation in [
            Expect::Installable,
            Expect::Installed,
            Expect::NotForeign,
            Expect::Unloaded,
        ] {
            assert_eq!(
                Query::Unknown.expect(expectation, Artifact::Absent, Platform::Linux),
                Err(Error::Manager)
            );
            assert_eq!(
                Query::Foreign.expect(expectation, Artifact::Absent, Platform::Linux),
                Err(Error::Foreign)
            );
            assert_eq!(
                Query::Absent.expect(expectation, Artifact::Foreign, Platform::Linux),
                Err(Error::Foreign)
            );
        }
        assert_eq!(
            Query::Absent.expect(Expect::Installed, Artifact::Absent, Platform::Linux),
            Err(Error::NotInstalled)
        );
    }
    #[test]
    fn installed_absence_differs_between_platforms() {
        assert_eq!(
            Query::Absent.expect(Expect::Installed, Artifact::Identical, Platform::Macos),
            Ok(())
        );
        assert_eq!(
            Query::Absent.expect(Expect::Installed, Artifact::Identical, Platform::Linux),
            Err(Error::NotInstalled)
        );
        for (state, status) in [
            (State::Running, Status::Running),
            (State::Reloading, Status::Running),
            (State::Starting, Status::Starting),
            (State::Stopping, Status::Stopping),
            (State::Stopped, Status::Stopped),
            (State::Failed, Status::Failed),
        ] {
            assert_eq!(
                Query::Ours(state).linux_status(Artifact::Identical),
                Ok(status)
            );
        }
    }
    #[test]
    fn gates_and_status_keep_unknown_errors_fixed() {
        for gate in [
            Gate::Always,
            Gate::Loaded,
            Gate::NotLoaded,
            Gate::ArtifactPresent,
        ] {
            assert_eq!(
                Query::Unknown.gate(gate, Artifact::Identical),
                Err(Error::Manager)
            );
            assert_eq!(
                Query::Foreign.gate(gate, Artifact::Identical),
                Err(Error::Foreign)
            );
        }
        assert_eq!(
            Query::Absent.gate(Gate::Loaded, Artifact::Absent),
            Ok(false)
        );
        assert_eq!(
            Query::Absent.gate(Gate::NotLoaded, Artifact::Absent),
            Ok(true)
        );
        assert_eq!(
            Query::Ours(State::Stopped).linux_status(Artifact::Identical),
            Ok(Status::Stopped)
        );
        assert_eq!(
            Query::Ours(State::Running).linux_status(Artifact::Absent),
            Err(Error::Manager)
        );
        assert_eq!(
            Query::Absent
                .linux_status(Artifact::Absent)
                .unwrap()
                .token(),
            "not-installed"
        );
    }
}

#[cfg(test)]
mod launchd_profile_tests {
    use super::*;
    const STATES: [State; 7] = [
        State::Running,
        State::Reloading,
        State::Starting,
        State::Stopping,
        State::Stopped,
        State::Failed,
        State::Loaded,
    ];
    const ABSENT_STDERR: &[u8] =
        b"Bad request.\nCould not find service \"com.arxcovo.arxd\" in domain for system\n";
    fn observe(termination: LaunchdTermination, stdout: &[u8], stderr: &[u8]) -> Query {
        launchd(LaunchdObservation {
            termination,
            stdout,
            stderr,
            stdout_complete: true,
            stderr_complete: true,
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }
    /// Synthetic loaded shape (tab-indented, nested blocks); not a native capture.
    fn print(path: &str, state: &str, program: &str, args: &[&str], extra: &str) -> Vec<u8> {
        let args: String = args.iter().map(|a| format!("\t\t{a}\n")).collect();
        format!(
            "system/com.arxcovo.arxd = {{\n\tactive count = 1\n\tpath = {path}\n\ttype = LaunchDaemon\n\tstate = {state}\n\n\tprogram = {program}\n\targuments = {{\n{args}\t}}\n\n\tdefault environment = {{\n\t\tPATH => /usr/bin:/bin:/usr/sbin:/sbin\n\t}}\n\n\tevent triggers = {{\n\t\tsynthetic => {{\n\t\t\tstate = 1\n\t\t\tpath = /nested\n\t\t}}\n\t}}\n\n\tdomain = system\n\tlast exit code = (never exited)\n{extra}}}\n"
        )
        .into_bytes()
    }
    fn ours(state: &str, extra: &str) -> Vec<u8> {
        print(PLIST_PATH, state, LAUNCHD_PROGRAM, LAUNCHD_ARGUMENTS, extra)
    }
    #[test]
    fn pinned_absence_requires_the_whole_native_tuple() {
        let exited = LaunchdTermination::Exited;
        assert_eq!(observe(exited(113), b"", ABSENT_STDERR), Query::Absent);
        for (code, stdout, stderr) in [
            (1, b"".as_slice(), ABSENT_STDERR),
            (0, b"", ABSENT_STDERR),
            (3, b"", ABSENT_STDERR),
            (113, b"x", ABSENT_STDERR),
            (113, b"", b""),
            (113, b"", b"Bad request.\n"),
            (
                113,
                b"",
                b"Bad request.\nCould not find service \"com.arxcovo.arxd\" in domain for system",
            ),
            (
                113,
                b"",
                b"Bad request.\nCould not find service \"com.arxcovo.other\" in domain for system\n",
            ),
            (
                113,
                b"",
                b"Bad request.\nCould not find service \"com.arxcovo.arxd\" in domain for user gui: 501\n",
            ),
            (
                113,
                b"",
                b"Bad request.\r\nCould not find service \"com.arxcovo.arxd\" in domain for system\r\n",
            ),
            (113, ABSENT_STDERR, b""),
        ] {
            assert_eq!(observe(exited(code), stdout, stderr), Query::Unknown);
        }
        for termination in [
            LaunchdTermination::Signalled,
            LaunchdTermination::Incomplete,
        ] {
            assert_eq!(observe(termination, b"", ABSENT_STDERR), Query::Unknown);
        }
    }
    #[test]
    fn incomplete_truncated_or_invalid_streams_fail_closed() {
        let loaded = ours("running", "");
        for (stdout, stderr, exit) in [
            (b"".as_slice(), ABSENT_STDERR, 113),
            (loaded.as_slice(), b"".as_slice(), 0),
        ] {
            let full = observe(LaunchdTermination::Exited(exit), stdout, stderr);
            assert_ne!(full, Query::Unknown);
            for flags in 0..16u8 {
                if flags == 0b0011 {
                    continue;
                }
                let q = launchd(LaunchdObservation {
                    termination: LaunchdTermination::Exited(exit),
                    stdout,
                    stderr,
                    stdout_complete: flags & 1 != 0,
                    stderr_complete: flags & 2 != 0,
                    stdout_truncated: flags & 4 != 0,
                    stderr_truncated: flags & 8 != 0,
                });
                assert_eq!(q, Query::Unknown, "flags {flags:04b}");
            }
        }
        let mut bad = loaded.clone();
        bad[3] = 0xff;
        assert_eq!(
            observe(LaunchdTermination::Exited(0), &bad, b""),
            Query::Unknown
        );
        let mut stderr = ABSENT_STDERR.to_vec();
        stderr[0] = 0xff;
        assert_eq!(
            observe(LaunchdTermination::Exited(113), b"", &stderr),
            Query::Unknown
        );
        let flood = [b'x'; LAUNCHD_STDOUT_CAP + 1];
        assert_eq!(
            observe(LaunchdTermination::Exited(0), &flood, b""),
            Query::Unknown
        );
        let flood = [b'x'; LAUNCHD_STDERR_CAP + 1];
        assert_eq!(
            observe(LaunchdTermination::Exited(113), b"", &flood),
            Query::Unknown
        );
    }
    #[test]
    fn loaded_states_map_and_unknown_vocabulary_refuses() {
        let exit0 = LaunchdTermination::Exited(0);
        for (state, want) in [
            ("running", Query::Ours(State::Running)),
            ("spawn scheduled", Query::Ours(State::Starting)),
            ("starting", Query::Ours(State::Starting)),
            ("not running", Query::Ours(State::Stopped)),
            ("waiting", Query::Ours(State::Stopped)),
            ("exited", Query::Unknown),
            ("Running", Query::Unknown),
            ("running ", Query::Unknown),
            ("", Query::Unknown),
        ] {
            assert_eq!(observe(exit0, &ours(state, ""), b""), want, "{state:?}");
        }
        // Loaded output with any stderr, or under another exit status, is Unknown.
        assert_eq!(
            observe(exit0, &ours("running", ""), b"warning\n"),
            Query::Unknown
        );
        for code in [1, 113] {
            assert_eq!(
                observe(LaunchdTermination::Exited(code), &ours("running", ""), b""),
                Query::Unknown
            );
        }
        // Nested `state =`/`path =` lines never stand in for top-level fields.
        assert_eq!(
            observe(exit0, &ours("running", ""), b""),
            Query::Ours(State::Running)
        );
    }
    #[test]
    fn foreign_identity_and_ambiguous_structure() {
        let exit0 = LaunchdTermination::Exited(0);
        let q = |bytes: Vec<u8>| observe(exit0, &bytes, b"");
        let args = LAUNCHD_ARGUMENTS;
        let program = LAUNCHD_PROGRAM;
        for foreign in [
            print(
                "/Library/LaunchDaemons/other.plist",
                "running",
                program,
                args,
                "",
            ),
            print(
                "/System/Library/LaunchDaemons/com.arxcovo.arxd.plist",
                "not running",
                program,
                args,
                "",
            ),
            print(PLIST_PATH, "running", "/usr/local/bin/arxd", args, ""),
            print(PLIST_PATH, "running", program, &[program, "service"], ""),
            print(
                PLIST_PATH,
                "running",
                program,
                &[program, "service", "run", "--x"],
                "",
            ),
            print(
                PLIST_PATH,
                "running",
                program,
                &["/tmp/arxd", "service", "run"],
                "",
            ),
        ] {
            assert_eq!(q(foreign), Query::Foreign);
        }
        for extra in [
            format!("\tpath = {PLIST_PATH}\n"),
            "\tstate = running\n".into(),
            format!("\tprogram = {program}\n"),
            "\targuments = {\n\t}\n".into(),
            "\tno separator\n".into(),
            "\t\tbad depth = 1\n".into(),
            "  spaced = 1\n".into(),
            "\tunclosed = {\n".into(),
            "}\n".into(),
        ] {
            assert_eq!(q(ours("running", &extra)), Query::Unknown, "{extra:?}");
        }
        let good = ours("running", "");
        let text = String::from_utf8(good.clone()).unwrap();
        for broken in [
            text.replacen(
                "system/com.arxcovo.arxd = {",
                "system/com.arxcovo.other = {",
                1,
            ),
            text.replacen(
                "system/com.arxcovo.arxd = {",
                "gui/501/com.arxcovo.arxd = {",
                1,
            ),
            text.replacen(&format!("\tpath = {PLIST_PATH}\n"), "", 1),
            text.replacen("\tstate = running\n", "", 1),
            text.replacen(&format!("\tprogram = {program}\n"), "", 1),
            text.replacen("\targuments = {\n", "\targs = {\n", 1),
            text.replacen("\t\tservice\n", "\t\tservice = {\n\t\t}\n", 1),
            text.replace('\n', "\r\n"),
            format!("{text}trailing\n"),
            format!("{text}system/com.arxcovo.arxd = {{\n}}\n"),
            text.trim_end_matches("}\n").to_owned(),
        ] {
            assert_eq!(q(broken.into_bytes()), Query::Unknown);
        }
        // A trailing newline is optional; blank lines between blocks are ignored.
        assert_eq!(
            q(good[..good.len() - 1].to_vec()),
            Query::Ours(State::Running)
        );
    }
    #[test]
    fn review_malformed_loaded_variants_are_unknown() {
        let exit0 = LaunchdTermination::Exited(0);
        let good = String::from_utf8(ours("running", "")).unwrap();
        assert_eq!(
            observe(exit0, good.as_bytes(), b""),
            Query::Ours(State::Running)
        );
        let program = LAUNCHD_PROGRAM;
        let argv_open = "\targuments = {\n";
        let mut variants = vec![
            // Whitespace-only or empty lines inside the argument vector.
            good.replacen(argv_open, &format!("{argv_open}\t\t\n"), 1),
            good.replacen(argv_open, &format!("{argv_open}\t\t \n"), 1),
            good.replacen(argv_open, &format!("{argv_open}\n"), 1),
            good.replacen("\t\trun\n", "\t\trun\n\t\t\n", 1),
            // Blank lines inside another nested block.
            good.replacen(
                "\tdefault environment = {\n",
                "\tdefault environment = {\n\n",
                1,
            ),
            // Whitespace-only line at top level.
            good.replacen("\ttype = LaunchDaemon\n", "\ttype = LaunchDaemon\n\t\n", 1),
            good.replacen("\ttype = LaunchDaemon\n", "\ttype = LaunchDaemon\n \n", 1),
            // Trailing blank or whitespace lines after the closing brace.
            format!("{good}\n"),
            format!("{good}\t\n"),
            format!("{good} \n"),
            format!("{good}\n\n"),
        ];
        // Block-valued required scalars, alone or duplicating the scalar.
        for (key, line) in [
            ("path", format!("\tpath = {PLIST_PATH}\n")),
            ("state", "\tstate = running\n".to_owned()),
            ("program", format!("\tprogram = {program}\n")),
        ] {
            let block = format!("\t{key} = {{\n\t\tx\n\t}}\n");
            variants.push(good.replacen(&line, &format!("{line}{block}"), 1));
            variants.push(good.replacen(&line, &format!("{block}{line}"), 1));
            variants.push(good.replacen(&line, &block, 1));
        }
        // Scalar-valued arguments, alone or duplicating the block.
        let argv_block = format!("{argv_open}\t\t{program}\n\t\tservice\n\t\trun\n\t}}\n");
        assert!(good.contains(&argv_block));
        for replacement in [
            format!("{argv_block}\targuments = {program}\n"),
            format!("\targuments = {program}\n{argv_block}"),
            format!("\targuments = {program}\n"),
        ] {
            variants.push(good.replacen(&argv_block, &replacement, 1));
        }
        for variant in variants {
            assert_ne!(variant, good);
            assert_eq!(
                observe(exit0, variant.as_bytes(), b""),
                Query::Unknown,
                "{variant:?}"
            );
        }
    }
    #[test]
    fn unknown_state_is_never_reported_as_foreign() {
        let exit0 = LaunchdTermination::Exited(0);
        for state in ["exited", "", "Running"] {
            for bytes in [
                print(
                    "/Library/LaunchDaemons/other.plist",
                    state,
                    LAUNCHD_PROGRAM,
                    LAUNCHD_ARGUMENTS,
                    "",
                ),
                print(
                    PLIST_PATH,
                    state,
                    "/usr/local/bin/arxd",
                    LAUNCHD_ARGUMENTS,
                    "",
                ),
                print(
                    PLIST_PATH,
                    state,
                    LAUNCHD_PROGRAM,
                    &["/tmp/arxd", "service", "run"],
                    "",
                ),
                print(
                    "/Library/LaunchDaemons/other.plist",
                    state,
                    "/x",
                    &["/x"],
                    "",
                ),
            ] {
                assert_eq!(observe(exit0, &bytes, b""), Query::Unknown, "{state:?}");
            }
        }
        // A recognized state with a mismatched identity is still Foreign.
        let foreign = print(
            "/Library/LaunchDaemons/other.plist",
            "waiting",
            "/x",
            &["/x"],
            "",
        );
        assert_eq!(observe(exit0, &foreign, b""), Query::Foreign);
    }
    #[test]
    fn probe_label_only_changes_the_expected_absence_label() {
        let stderr =
            b"Bad request.\nCould not find service \"com.arxcovo.nonexistent-probe\" in domain for system\n";
        let o = LaunchdObservation {
            termination: LaunchdTermination::Exited(113),
            stdout: b"",
            stderr,
            stdout_complete: true,
            stderr_complete: true,
            stdout_truncated: false,
            stderr_truncated: false,
        };
        assert_eq!(
            launchd_probe(&o, "com.arxcovo.nonexistent-probe"),
            Query::Absent
        );
        assert_eq!(launchd(o), Query::Unknown);
    }
    #[test]
    fn arbitrary_observations_stay_unknown() {
        for termination in [
            LaunchdTermination::Exited(0),
            LaunchdTermination::Exited(1),
            LaunchdTermination::Exited(113),
            LaunchdTermination::Signalled,
            LaunchdTermination::Incomplete,
        ] {
            for stdout in [
                b"".as_slice(),
                b"synthetic absence",
                b"state = running",
                &[255, 0],
                &[b'x'; 65537],
            ] {
                for stderr in [b"".as_slice(), b"synthetic absence", &[b'x'; 4097]] {
                    for flags in 0..16 {
                        let q = launchd(LaunchdObservation {
                            termination,
                            stdout,
                            stderr,
                            stdout_complete: flags & 1 != 0,
                            stderr_complete: flags & 2 != 0,
                            stdout_truncated: flags & 4 != 0,
                            stderr_truncated: flags & 8 != 0,
                        });
                        assert!(q == Query::Unknown, "arbitrary-observation-refused");
                    }
                }
            }
        }
    }
    #[test]
    fn pure_macos_state_and_status_matrix() {
        let mut queries = vec![Query::Absent, Query::Unknown, Query::Foreign];
        queries.extend(STATES.map(Query::Ours));
        for q in queries {
            for a in [Artifact::Absent, Artifact::Identical, Artifact::Foreign] {
                let expected = match (a, q) {
                    (Artifact::Absent, Query::Absent) => Ok(Status::NotInstalled),
                    (Artifact::Identical, Query::Absent) => Ok(Status::Stopped),
                    (Artifact::Identical, Query::Ours(State::Running)) => Ok(Status::Running),
                    (Artifact::Identical, Query::Ours(State::Starting)) => Ok(Status::Starting),
                    (Artifact::Identical, Query::Ours(State::Stopped)) => Ok(Status::Stopped),
                    _ => Err(Error::Manager),
                };
                assert_eq!(q.macos_status(a), expected, "exhaustive-macos-status");
            }
        }
        for state in STATES {
            let q = Query::Ours(state);
            for a in [Artifact::Absent, Artifact::Identical] {
                assert_eq!(
                    q.expect(Expect::Installable, a, Platform::Macos),
                    Err(Error::Active),
                    "mac-all-ours-block-install"
                );
                assert_eq!(
                    q.expect(Expect::Unloaded, a, Platform::Macos),
                    Err(Error::Manager),
                    "mac-all-ours-block-unloaded"
                );
                assert_eq!(q.gate(Gate::Loaded, a), Ok(true));
                assert_eq!(q.gate(Gate::NotLoaded, a), Ok(false));
                let inactive = matches!(state, State::Stopped | State::Failed);
                assert_eq!(
                    q.expect(Expect::Installable, a, Platform::Linux),
                    if inactive { Ok(()) } else { Err(Error::Active) }
                );
                assert_eq!(
                    q.expect(Expect::Unloaded, a, Platform::Linux),
                    if inactive {
                        Ok(())
                    } else {
                        Err(Error::Manager)
                    }
                );
            }
        }
        for q in [Query::Unknown, Query::Foreign] {
            for platform in [Platform::Linux, Platform::Macos] {
                for expectation in [
                    Expect::Installable,
                    Expect::Installed,
                    Expect::NotForeign,
                    Expect::Unloaded,
                ] {
                    assert!(
                        q.expect(expectation, Artifact::Identical, platform)
                            .is_err(),
                        "uncertain-expect-refuses"
                    );
                }
            }
            for gate in [
                Gate::Always,
                Gate::Loaded,
                Gate::NotLoaded,
                Gate::ArtifactPresent,
            ] {
                assert!(
                    q.gate(gate, Artifact::Identical).is_err(),
                    "uncertain-gate-refuses"
                );
            }
        }
        for a in [Artifact::Absent, Artifact::Identical] {
            for e in [Expect::Installable, Expect::Unloaded] {
                assert_eq!(Query::Absent.expect(e, a, Platform::Macos), Ok(()));
            }
        }
    }
}
