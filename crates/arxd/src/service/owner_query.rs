//! Bounded, output-free manager observations for ADR 0014.
//! Native launchd absence/state fixtures are deliberately not pinned here.
use super::layout::{Error, Platform};
pub const DROPIN_PATH: &str = "/etc/systemd/system/arxd.service.d/50-arxcovo-standard-rights.conf";
pub const SYSTEMD_QUERY: &[&str] = &[
    "show",
    "--property=LoadState,ActiveState,FragmentPath,DropInPaths",
    "arxd.service",
];
pub const LAUNCHD_QUERY: &[&str] = &["print", "system/com.arxcovo.arxd"];
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
    fn active(self) -> bool {
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
            Expect::Installable if self.active() => Err(Error::Active),
            Expect::Installed
                if primary != Artifact::Identical
                    || (platform == Platform::Linux && self == Self::Absent) =>
            {
                Err(Error::NotInstalled)
            }
            Expect::Unloaded if self.active() => Err(Error::Manager),
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
    if (!fragment.is_empty() && fragment != "/etc/systemd/system/arxd.service")
        || (!dropin.is_empty() && dropin != DROPIN_PATH)
    {
        return Query::Foreign;
    }
    match load {
        "not-found" if fragment.is_empty() && dropin.is_empty() => Query::Absent,
        "loaded" if fragment == "/etc/systemd/system/arxd.service" => Query::Ours(state),
        _ => Query::Unknown,
    }
}
/// No native fixture currently establishes launchd's output grammar, absence
/// exit status or state vocabulary. Do not infer any of them from an exit code.
pub fn launchd(_success: bool, _stdout: &[u8]) -> Query {
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
        let unit = "/etc/systemd/system/arxd.service";
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
        for success in [false, true] {
            for bytes in [&b""[..], &b"path = /foreign"[..], &good[..]] {
                assert_eq!(launchd(success, bytes), Query::Unknown);
            }
        }
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
