//! Bounded, output-free manager observations for ADR 0014.
//! Native launchd absence/state fixtures are deliberately not pinned here.
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
/// No native fixture currently establishes launchd's output grammar, absence
/// exit status or state vocabulary. Do not infer any of them from an exit code.
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
/// Empty accept-set: no output grammar or absence tuple has been reviewed.
pub fn launchd(_observation: LaunchdObservation<'_>) -> Query {
    Query::Unknown
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
mod empty_profile_tests {
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
    #[test]
    fn pure_empty_profile_every_observation_unknown() {
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
                        assert!(
                            q == Query::Unknown,
                            "empty-profile-refuses-every-observation"
                        );
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
