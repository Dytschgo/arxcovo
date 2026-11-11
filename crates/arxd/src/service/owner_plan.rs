//! Owner-only operation plans; deliberately no CLI, MCP route or native executor.
use super::owner_transactions::BatchObservation;
use super::{
    config::{Resolver, Rights, ServiceConfig},
    layout::{Error, Layout, Platform, Policy},
};
use std::path::{Path, PathBuf};
pub const UNIT: &[u8] = include_bytes!("../../../../packaging/linux/arxd.service");
pub const DROPIN: &[u8] =
    include_bytes!("../../../../packaging/linux/50-arxcovo-standard-rights.conf");
pub const PLIST: &[u8] = include_bytes!("../../../../packaging/macos/com.arxcovo.arxd.plist");
pub use super::owner_query::{DROPIN_PATH, Expect, Gate};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstantGate {
    Always,
    UnitAbsent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    Start,
    Stop,
    Status,
    Uninstall,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scm {
    Register,
    Start,
    Stop,
    Status,
    Unregister,
}
/// Complete fixed SCM registration contract, consumed by the future adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub name: &'static str,
    pub image: PathBuf,
    pub args: [&'static str; 2],
    pub account: &'static str,
    pub own_process: bool,
    pub delayed_auto_start: bool,
    pub unrestricted_service_sid: bool,
    pub privileges: [&'static str; 4],
    pub preshutdown_ms: u32,
    pub restart_delays_ms: [u32; 2],
    pub reset_seconds: u32,
    pub restart_non_crash: bool,
}
fn registration(l: &Layout) -> Registration {
    Registration {
        name: "arxd",
        image: l.binary().into(),
        args: ["service", "run"],
        account: "LocalSystem",
        own_process: true,
        delayed_auto_start: true,
        unrestricted_service_sid: true,
        privileges: [
            "SeTcbPrivilege",
            "SeAssignPrimaryTokenPrivilege",
            "SeIncreaseQuotaPrivilege",
            "SeChangeNotifyPrivilege",
        ],
        preshutdown_ms: 30000,
        restart_delays_ms: [5000, 30000],
        reset_seconds: 86400,
        restart_non_crash: false,
    }
}
/// Adapter must refuse wrong existing owner/mode, unsafe ancestors and reparse
/// points. No step authorizes taking over or repairing an existing object.
#[derive(Clone, PartialEq, Eq)]
pub enum Operation {
    Expect(Expect),
    StartPreflight,
    ProvisionState,
    Verify {
        path: PathBuf,
        policy: Policy,
        required: bool,
    },
    EnsureDirectory {
        path: PathBuf,
        policy: Policy,
    },
    /// Exclusive create; existing bytes AND protection must be identical.
    Constant {
        path: PathBuf,
        bytes: &'static [u8],
        when: ConstantGate,
    },
    /// Same-directory private temp, explicit service owner/protection, flush,
    /// compare expected old bytes (None = exclusive create), atomic rename.
    /// Must not replace an object changed since validation; no symlink following.
    AtomicConfig {
        path: PathBuf,
        bytes: Vec<u8>,
        expected: Option<Vec<u8>>,
    },
    BatchRight {
        sid: String,
        add: bool,
    },
    RemoveConstant {
        path: PathBuf,
        expected: &'static [u8],
    },
    Scm {
        action: Scm,
        registration: Registration,
    },
    Command {
        program: &'static str,
        args: Vec<&'static str>,
        when: Gate,
    },
}
/// Fixed output only. Intentionally does not Debug-print Operation payloads.
pub struct Plan {
    operations: Vec<Operation>,
    transaction_only: bool,
}
/// This evidence must come from an owner-local native adapter, never MCP input.
pub struct OwnerContext<'a> {
    pub admin: bool,
    pub running_binary: &'a Path,
}
pub trait OsManager {
    fn apply(&mut self, operation: &Operation) -> Result<(), Error>;
}
impl Plan {
    #[cfg(test)]
    pub(super) fn test_operations_mut(&mut self) -> &mut [Operation] {
        &mut self.operations
    }
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }
    /// Stops on the first error. No automatic rollback/takeover after partial failure.
    pub fn execute(&self, manager: &mut impl OsManager) -> Result<(), Error> {
        if self.transaction_only {
            return Err(Error::NativeAdapterMissing);
        }
        for op in &self.operations {
            manager.apply(op)?;
        }
        Ok(())
    }
}
pub struct MissingNativeAdapter;
impl OsManager for MissingNativeAdapter {
    fn apply(&mut self, _: &Operation) -> Result<(), Error> {
        Err(Error::NativeAdapterMissing)
    }
}
fn authorize(l: &Layout, c: &OwnerContext<'_>, install: bool) -> Result<(), Error> {
    if !c.admin {
        return Err(Error::OwnerRequired);
    }
    if install && c.running_binary != l.binary() {
        return Err(Error::Layout);
    }
    Ok(())
}
fn guards(l: &Layout) -> Vec<Operation> {
    let mut v = vec![Operation::Verify {
        path: l.binary().into(),
        policy: Policy::BinaryIntegrity,
        required: true,
    }];
    for path in l.private_dirs() {
        v.push(Operation::Verify {
            path: path.clone(),
            policy: Policy::PrivateDirectory,
            required: false,
        });
    }
    for (path, policy) in [
        (l.config(), Policy::PrivateFile),
        (l.state(), Policy::PrivateDirectory),
    ] {
        v.push(Operation::Verify {
            path: path.into(),
            policy,
            required: false,
        });
    }
    if l.platform() != Platform::Windows {
        v.push(Operation::Verify {
            path: l.endpoint().parent().unwrap().into(),
            policy: Policy::EndpointDirectory,
            required: false,
        });
    }
    v
}
fn manager(l: &Layout, action: Scm) -> Operation {
    match l.platform() {
        Platform::Windows => Operation::Scm {
            action,
            registration: registration(l),
        },
        Platform::Linux => Operation::Command {
            program: "/usr/bin/systemctl",
            when: Gate::Always,
            args: vec![
                match action {
                    Scm::Start => "start",
                    Scm::Stop => "stop",
                    Scm::Status => "is-active",
                    _ => unreachable!(),
                },
                "arxd.service",
            ],
        },
        Platform::Macos => Operation::Command {
            program: "/bin/launchctl",
            when: Gate::Always,
            args: match action {
                Scm::Start => vec!["bootstrap", "system", super::owner_query::PLIST_PATH],
                Scm::Stop => vec!["bootout", super::owner_query::LAUNCHD_TARGET],
                Scm::Status => vec!["print", super::owner_query::LAUNCHD_TARGET],
                _ => unreachable!(),
            },
        },
    }
}
fn reload() -> Operation {
    Operation::Command {
        program: "/usr/bin/systemctl",
        args: vec!["daemon-reload"],
        when: Gate::Always,
    }
}
fn unix_command(l: &Layout, verb: &'static str, when: Gate) -> Operation {
    let (program, args) = if l.platform() == Platform::Linux {
        ("/usr/bin/systemctl", vec![verb, "arxd.service"])
    } else if verb == "bootstrap" {
        (
            "/bin/launchctl",
            vec![verb, "system", super::owner_query::PLIST_PATH],
        )
    } else {
        (
            "/bin/launchctl",
            vec![verb, super::owner_query::LAUNCHD_TARGET],
        )
    };
    Operation::Command {
        program,
        args,
        when,
    }
}
fn unix_plan(l: &Layout, action: Action) -> Result<Plan, Error> {
    let mut operations = guards(l);
    let linux = l.platform() == Platform::Linux;
    let stop = if linux { "stop" } else { "bootout" };
    match action {
        Action::Status => return Err(Error::Manager), // read-only owner_query API
        Action::Install => {
            operations.push(Operation::Expect(Expect::Installable));
            for path in l.private_dirs() {
                operations.push(Operation::EnsureDirectory {
                    path: path.clone(),
                    policy: Policy::PrivateDirectory,
                });
            }
            operations.push(Operation::ProvisionState);
            if linux {
                operations.push(Operation::Constant {
                    path: DROPIN_PATH.into(),
                    bytes: DROPIN,
                    when: ConstantGate::UnitAbsent,
                });
                operations.push(Operation::Constant {
                    path: l.artifact().into(),
                    bytes: UNIT,
                    when: ConstantGate::Always,
                });
                operations.push(reload());
                operations.push(unix_command(l, "disable", Gate::Always));
            } else {
                operations.push(unix_command(l, "disable", Gate::Always));
                operations.push(Operation::Constant {
                    path: l.artifact().into(),
                    bytes: PLIST,
                    when: ConstantGate::Always,
                });
            }
            operations.push(Operation::Expect(Expect::Unloaded));
        }
        Action::Start => {
            operations.push(Operation::StartPreflight);
            operations.push(Operation::Expect(Expect::Installed));
            operations.push(unix_command(l, "enable", Gate::Always));
            operations.push(unix_command(
                l,
                if linux { "start" } else { "bootstrap" },
                if linux { Gate::Always } else { Gate::NotLoaded },
            ));
        }
        Action::Stop | Action::Uninstall => {
            let uninstall = action == Action::Uninstall;
            operations.push(Operation::Expect(if uninstall {
                Expect::NotForeign
            } else {
                Expect::Installed
            }));
            operations.push(unix_command(
                l,
                "disable",
                if uninstall && linux {
                    Gate::ArtifactPresent
                } else {
                    Gate::Always
                },
            ));
            operations.push(unix_command(
                l,
                stop,
                if uninstall || !linux {
                    Gate::Loaded
                } else {
                    Gate::Always
                },
            ));
            operations.push(Operation::Expect(Expect::Unloaded));
            if uninstall {
                operations.push(Operation::RemoveConstant {
                    path: l.artifact().into(),
                    expected: if linux { UNIT } else { PLIST },
                });
                if linux {
                    operations.push(Operation::RemoveConstant {
                        path: DROPIN_PATH.into(),
                        expected: DROPIN,
                    });
                    operations.push(reload());
                }
            }
        }
    }
    Ok(Plan {
        operations,
        transaction_only: false,
    })
}
pub fn owner_plan(l: &Layout, c: &OwnerContext<'_>, action: Action) -> Result<Plan, Error> {
    authorize(
        l,
        c,
        action == Action::Install
            || (l.platform() == Platform::Windows && action == Action::Uninstall),
    )?;
    if l.platform() != Platform::Windows {
        return unix_plan(l, action);
    }
    let mut operations = guards(l);
    match action {
        Action::Install => {
            for path in l.private_dirs() {
                operations.push(Operation::EnsureDirectory {
                    path: path.clone(),
                    policy: Policy::PrivateDirectory,
                });
            }
            match l.platform() {
                Platform::Windows => operations.push(Operation::Scm {
                    action: Scm::Register,
                    registration: registration(l),
                }),
                Platform::Linux => {
                    operations.push(Operation::Constant {
                        path: l.artifact().into(),
                        bytes: UNIT,
                        when: ConstantGate::Always,
                    });
                    operations.push(Operation::Constant {
                        path: DROPIN_PATH.into(),
                        bytes: DROPIN,
                        when: ConstantGate::Always,
                    });
                    operations.push(reload());
                }
                Platform::Macos => operations.push(Operation::Constant {
                    path: l.artifact().into(),
                    bytes: PLIST,
                    when: ConstantGate::Always,
                }),
            }
        }
        Action::Start => operations.push(manager(l, Scm::Start)),
        Action::Stop => operations.push(manager(l, Scm::Stop)),
        Action::Status => operations.push(manager(l, Scm::Status)),
        Action::Uninstall => {
            operations.push(manager(l, Scm::Stop));
            match l.platform() {
                Platform::Windows => operations.push(Operation::Scm {
                    action: Scm::Unregister,
                    registration: registration(l),
                }),
                Platform::Linux => {
                    operations.push(Operation::RemoveConstant {
                        path: l.artifact().into(),
                        expected: UNIT,
                    });
                    operations.push(Operation::RemoveConstant {
                        path: DROPIN_PATH.into(),
                        expected: DROPIN,
                    });
                    operations.push(reload());
                }
                Platform::Macos => operations.push(Operation::RemoveConstant {
                    path: l.artifact().into(),
                    expected: PLIST,
                }),
            }
        }
    }
    Ok(Plan {
        operations,
        transaction_only: l.platform() == Platform::Windows && action == Action::Uninstall,
    })
}
/// Windows requires Some(observe_batch(...)) from the locked private session;
/// Unix requires None and has no LSA/provenance dependency. The config mirror
/// never authorizes ownership. Changing the build pin requires uninstall.
pub fn configure_plan(
    l: &Layout,
    c: &OwnerContext<'_>,
    config: &ServiceConfig,
    previous: Option<&ServiceConfig>,
    batch: Option<BatchObservation>,
    resolver: &impl Resolver,
) -> Result<Plan, Error> {
    authorize(l, c, l.platform() == Platform::Windows)?;
    if (l.platform() == Platform::Windows) != batch.is_some() {
        return Err(Error::Config);
    }
    config.revalidate(l, resolver)?;
    if let Some(old) = previous {
        old.revalidate(l, resolver)?;
        if old.build_identity() != config.build_identity() {
            return Err(Error::Identity);
        }
    }
    let mut operations = guards(l);
    let granted = l.platform() == Platform::Windows
        && batch.is_some_and(|b| !b.present() || b.recorded_grant());
    if l.platform() == Platform::Windows && batch.is_some_and(|b| !b.present()) {
        operations.push(Operation::BatchRight {
            sid: config.build_identity().into(),
            add: true,
        });
    }
    operations.push(Operation::AtomicConfig {
        path: l.config().into(),
        bytes: config.serialized(granted)?,
        expected: previous
            .map(|v| v.serialized(v.granted_batch_right()))
            .transpose()?,
    });
    if l.platform() == Platform::Linux {
        operations.push(if config.rights() == Rights::Standard {
            Operation::Constant {
                path: DROPIN_PATH.into(),
                bytes: DROPIN,
                when: ConstantGate::Always,
            }
        } else {
            Operation::RemoveConstant {
                path: DROPIN_PATH.into(),
                expected: DROPIN,
            }
        });
        operations.push(reload());
    }
    Ok(Plan {
        operations,
        transaction_only: l.platform() == Platform::Windows,
    })
}
/// Windows ignores config/resolver: use owner_plan(Uninstall) without a config,
/// then uninstall_right under the session lock. Unix retains revalidation.
pub fn uninstall_plan(
    l: &Layout,
    c: &OwnerContext<'_>,
    previous: &ServiceConfig,
    resolver: &impl Resolver,
) -> Result<Plan, Error> {
    if l.platform() != Platform::Windows {
        previous.revalidate(l, resolver)?;
    }
    owner_plan(l, c, Action::Uninstall)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::config::tests::{FakeResolver, fixture, parse};
    #[derive(Default)]
    struct RecordingOsManager {
        calls: Vec<Operation>,
        fail_at: Option<usize>,
    }
    impl OsManager for RecordingOsManager {
        fn apply(&mut self, op: &Operation) -> Result<(), Error> {
            if self.fail_at == Some(self.calls.len()) {
                return Err(Error::Manager);
            }
            self.calls.push(op.clone());
            Ok(())
        }
    }
    /// Scripted manager MODEL only: no native execution or boot evidence.
    #[test]
    fn lifecycle_reinstall_boot_model() {
        use super::super::owner_query::{Artifact, Query, State};
        struct Model {
            os: Platform,
            artifact: Artifact,
            query: Query,
            boot: bool,
            retained: Vec<u8>,
        }
        impl OsManager for Model {
            fn apply(&mut self, op: &Operation) -> Result<(), Error> {
                match op {
                    Operation::Expect(e) => self.query.expect(*e, self.artifact, self.os)?,
                    Operation::Constant { path, when, .. }
                        if (*when == ConstantGate::Always || self.artifact == Artifact::Absent)
                            && path != Path::new(DROPIN_PATH) =>
                    {
                        self.artifact = Artifact::Identical;
                    }
                    Operation::Command { args, when, .. }
                        if self.query.gate(*when, self.artifact)? =>
                    {
                        match args[0] {
                            "enable" => self.boot = true,
                            "disable" => self.boot = false,
                            "start" | "bootstrap" => {
                                self.query = Query::Ours(if self.os == Platform::Linux {
                                    State::Running
                                } else {
                                    State::Loaded
                                })
                            }
                            "stop" | "bootout" => {
                                self.query = if self.os == Platform::Linux {
                                    Query::Ours(State::Stopped)
                                } else {
                                    Query::Absent
                                }
                            }
                            "daemon-reload" => {
                                self.query = if self.artifact == Artifact::Absent {
                                    Query::Absent
                                } else {
                                    Query::Ours(State::Stopped)
                                }
                            }
                            _ => return Err(Error::Manager),
                        }
                    }
                    Operation::RemoveConstant { path, .. } if path != Path::new(DROPIN_PATH) => {
                        self.artifact = Artifact::Absent
                    }
                    _ => {}
                }
                Ok(())
            }
        }
        for os in [Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            let c = OwnerContext {
                admin: true,
                running_binary: l.binary(),
            };
            let mut m = Model {
                os,
                artifact: Artifact::Absent,
                query: Query::Absent,
                boot: true,
                retained: b"retained database".to_vec(),
            };
            for (action, boot) in [
                (Action::Install, false),
                (Action::Start, true),
                (Action::Start, true),
                (Action::Stop, false),
                (Action::Stop, false),
                (Action::Install, false),
                (Action::Uninstall, false),
                (Action::Install, false),
                (Action::Start, true),
                (Action::Uninstall, false),
            ] {
                owner_plan(&l, &c, action).unwrap().execute(&mut m).unwrap();
                assert_eq!(m.boot, boot);
                assert_eq!(m.retained, b"retained database");
            }
            owner_plan(&l, &c, Action::Install)
                .unwrap()
                .execute(&mut m)
                .unwrap();
            owner_plan(&l, &c, Action::Start)
                .unwrap()
                .execute(&mut m)
                .unwrap();
            assert_eq!(
                owner_plan(&l, &c, Action::Install).unwrap().execute(&mut m),
                Err(Error::Active)
            );
            assert!(m.boot);
        }
    }
    #[test]
    fn unix_exact_typed_plans() {
        for os in [Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            let c = OwnerContext {
                admin: true,
                running_binary: l.binary(),
            };
            let linux = os == Platform::Linux;
            for action in [
                Action::Install,
                Action::Start,
                Action::Stop,
                Action::Uninstall,
            ] {
                let p = owner_plan(&l, &c, action).unwrap();
                let tail = &p.operations()[guards(&l).len()..];
                let mut expected = Vec::new();
                match action {
                    Action::Install => {
                        expected.push(Operation::Expect(Expect::Installable));
                        for path in l.private_dirs() {
                            expected.push(Operation::EnsureDirectory {
                                path: path.clone(),
                                policy: Policy::PrivateDirectory,
                            });
                        }
                        expected.push(Operation::ProvisionState);
                        if linux {
                            expected.push(Operation::Constant {
                                path: DROPIN_PATH.into(),
                                bytes: DROPIN,
                                when: ConstantGate::UnitAbsent,
                            });
                            expected.push(Operation::Constant {
                                path: l.artifact().into(),
                                bytes: UNIT,
                                when: ConstantGate::Always,
                            });
                            expected.push(reload());
                            expected.push(unix_command(&l, "disable", Gate::Always));
                        } else {
                            expected.push(unix_command(&l, "disable", Gate::Always));
                            expected.push(Operation::Constant {
                                path: l.artifact().into(),
                                bytes: PLIST,
                                when: ConstantGate::Always,
                            });
                        }
                        expected.push(Operation::Expect(Expect::Unloaded));
                    }
                    Action::Start => {
                        expected.extend([
                            Operation::StartPreflight,
                            Operation::Expect(Expect::Installed),
                            unix_command(&l, "enable", Gate::Always),
                            unix_command(
                                &l,
                                if linux { "start" } else { "bootstrap" },
                                if linux { Gate::Always } else { Gate::NotLoaded },
                            ),
                        ]);
                    }
                    Action::Stop | Action::Uninstall => {
                        let uninstall = action == Action::Uninstall;
                        expected.extend([
                            Operation::Expect(if uninstall {
                                Expect::NotForeign
                            } else {
                                Expect::Installed
                            }),
                            unix_command(
                                &l,
                                "disable",
                                if uninstall && linux {
                                    Gate::ArtifactPresent
                                } else {
                                    Gate::Always
                                },
                            ),
                            unix_command(
                                &l,
                                if linux { "stop" } else { "bootout" },
                                if uninstall || !linux {
                                    Gate::Loaded
                                } else {
                                    Gate::Always
                                },
                            ),
                            Operation::Expect(Expect::Unloaded),
                        ]);
                        if uninstall {
                            expected.push(Operation::RemoveConstant {
                                path: l.artifact().into(),
                                expected: if linux { UNIT } else { PLIST },
                            });
                            if linux {
                                expected.push(Operation::RemoveConstant {
                                    path: DROPIN_PATH.into(),
                                    expected: DROPIN,
                                });
                                expected.push(reload());
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                assert!(tail == expected);
                assert!(!p.operations().iter().any(|op| matches!(
                    op,
                    Operation::EnsureDirectory {
                        policy: Policy::EndpointDirectory,
                        ..
                    }
                )));
                assert!(p.operations().iter().any(|op| matches!(
                    op,
                    Operation::Verify {
                        policy: Policy::EndpointDirectory,
                        required: false,
                        ..
                    }
                )));
            }
            assert!(matches!(
                owner_plan(&l, &c, Action::Status),
                Err(Error::Manager)
            ));
        }
    }
    #[test]
    fn install_never_starts_and_failure_stops() {
        for os in [Platform::Windows, Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            let c = OwnerContext {
                admin: true,
                running_binary: l.binary(),
            };
            let p = owner_plan(&l, &c, Action::Install).unwrap();
            assert!(!p.operations().iter().any(|o|matches!(o,Operation::Scm {action:Scm::Start,..}) || matches!(o,Operation::Command {args,..} if args.contains(&"start") || args.contains(&"bootstrap"))));
            let mut m = RecordingOsManager {
                fail_at: Some(1),
                ..Default::default()
            };
            assert_eq!(p.execute(&mut m), Err(Error::Manager));
            assert_eq!(m.calls.len(), 1);
            let mut m = RecordingOsManager::default();
            p.execute(&mut m).unwrap();
            assert_eq!(m.calls.len(), p.operations.len());
            assert!(
                owner_plan(
                    &l,
                    &OwnerContext {
                        admin: false,
                        running_binary: l.binary()
                    },
                    Action::Install
                )
                .is_err()
            );
            assert!(
                owner_plan(
                    &l,
                    &OwnerContext {
                        admin: true,
                        running_binary: Path::new("injected")
                    },
                    Action::Install
                )
                .is_err()
            );
        }
    }
    #[test]
    fn fixed_vectors_and_uninstall_preserves_user_objects() {
        for os in [Platform::Windows, Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            let c = OwnerContext {
                admin: true,
                running_binary: l.binary(),
            };
            for (action, scm) in [
                (Action::Start, Scm::Start),
                (Action::Stop, Scm::Stop),
                (Action::Status, Scm::Status),
            ] {
                if os == Platform::Windows {
                    let p = owner_plan(&l, &c, action).unwrap();
                    assert!(p.operations.last() == Some(&manager(&l, scm)));
                }
            }
            let p = owner_plan(&l, &c, Action::Uninstall).unwrap();
            assert!(p.operations.iter().all(|o| match o {
                Operation::RemoveConstant { path, .. } => path != l.config() && path != l.state(),
                Operation::AtomicConfig { .. }
                | Operation::EnsureDirectory { .. }
                | Operation::BatchRight { .. } => false,
                _ => true,
            }));
        }
        let l = Layout::for_test(Platform::Linux);
        assert!(
            manager(&l, Scm::Start)
                == Operation::Command {
                    program: "/usr/bin/systemctl",
                    args: vec!["start", "arxd.service"],
                    when: Gate::Always
                }
        );
        let reg = registration(&Layout::for_test(Platform::Windows));
        assert_eq!(reg.args, ["service", "run"]);
        assert_eq!(reg.account, "LocalSystem");
        assert!(!reg.restart_non_crash);
    }
    #[test]
    fn configure_serializes_and_does_not_overwrite_without_expected_bytes() {
        let l = Layout::for_test(Platform::Linux);
        let c = OwnerContext {
            admin: true,
            running_binary: l.binary(),
        };
        let config = parse(fixture(), &FakeResolver::default()).unwrap();
        let p = configure_plan(&l, &c, &config, None, None, &FakeResolver::default()).unwrap();
        let atomic = p
            .operations
            .iter()
            .find(|o| matches!(o, Operation::AtomicConfig { .. }))
            .unwrap();
        if let Operation::AtomicConfig {
            bytes, expected, ..
        } = atomic
        {
            assert!(expected.is_none());
            assert!(ServiceConfig::parse(bytes, &l, &FakeResolver::default()).is_ok());
        }
        assert!(p.operations.last() == Some(&reload()));
        let mut v = fixture();
        v["rights"] = "admin".into();
        let admin = FakeResolver {
            admin: true,
            ..Default::default()
        };
        let admin_config = parse(v, &admin).unwrap();
        let admin_plan = configure_plan(&l, &c, &admin_config, None, None, &admin).unwrap();
        assert!(admin_plan.operations.iter().any(|o|matches!(o,Operation::RemoveConstant {path,expected} if path==Path::new(DROPIN_PATH) && *expected==DROPIN)));
        let mut m = RecordingOsManager {
            fail_at: Some(0),
            ..Default::default()
        };
        assert!(p.execute(&mut m).is_err());
        assert!(m.calls.is_empty());
        assert!(
            configure_plan(
                &l,
                &c,
                &config,
                None,
                None,
                &FakeResolver {
                    changed: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    /// In-memory OS semantics, not evidence of native ACL/atomicity correctness.
    #[derive(Default)]
    struct ModelManager {
        files: std::collections::BTreeMap<PathBuf, Vec<u8>>,
        unsafe_path: Option<PathBuf>,
        calls: usize,
    }
    impl OsManager for ModelManager {
        fn apply(&mut self, op: &Operation) -> Result<(), Error> {
            self.calls += 1;
            match op {
                Operation::Verify { path, .. } if self.unsafe_path.as_ref() == Some(path) => {
                    Err(Error::Permissions)
                }
                Operation::Constant { path, bytes, .. } => {
                    if self.files.get(path).is_some_and(|old| old != bytes) {
                        return Err(Error::Permissions);
                    }
                    self.files.insert(path.clone(), bytes.to_vec());
                    Ok(())
                }
                Operation::AtomicConfig {
                    path,
                    bytes,
                    expected,
                } => {
                    if self.files.get(path) != expected.as_ref() {
                        return Err(Error::Permissions);
                    }
                    self.files.insert(path.clone(), bytes.clone());
                    Ok(())
                }
                _ => Ok(()),
            }
        }
    }
    #[test]
    fn squatted_layout_and_wrong_owner_refuse_before_mutations() {
        let l = Layout::for_test(Platform::Linux);
        let c = OwnerContext {
            admin: true,
            running_binary: l.binary(),
        };
        for path in [l.private_dirs()[0].as_path(), l.state(), l.config()] {
            let mut m = ModelManager {
                unsafe_path: Some(path.into()),
                ..Default::default()
            };
            assert_eq!(
                owner_plan(&l, &c, Action::Install).unwrap().execute(&mut m),
                Err(Error::Permissions)
            );
            assert!(m.files.is_empty());
        }
        let mut m = ModelManager::default();
        m.files
            .insert(l.artifact().into(), b"foreign unit".to_vec());
        assert_eq!(
            owner_plan(&l, &c, Action::Install).unwrap().execute(&mut m),
            Err(Error::Permissions)
        );
        assert_eq!(m.files[l.artifact()], b"foreign unit");
    }
    #[test]
    fn atomic_plan_refuses_existing_and_concurrent_config_changes() {
        let l = Layout::for_test(Platform::Linux);
        let c = OwnerContext {
            admin: true,
            running_binary: l.binary(),
        };
        let config = parse(fixture(), &FakeResolver::default()).unwrap();
        let p = configure_plan(&l, &c, &config, None, None, &FakeResolver::default()).unwrap();
        let mut m = ModelManager::default();
        p.execute(&mut m).unwrap();
        let old = m.files[l.config()].clone();
        assert_eq!(p.execute(&mut m), Err(Error::Permissions));
        assert_eq!(m.files[l.config()], old);
        let update = configure_plan(
            &l,
            &c,
            &config,
            Some(&config),
            None,
            &FakeResolver::default(),
        )
        .unwrap();
        update.execute(&mut m).unwrap();
        m.files
            .insert(l.config().into(), b"concurrent edit".to_vec());
        assert_eq!(update.execute(&mut m), Err(Error::Permissions));
        assert_eq!(m.files[l.config()], b"concurrent edit");
        assert_eq!(
            p.execute(&mut MissingNativeAdapter),
            Err(Error::NativeAdapterMissing)
        );
    }
    #[test]
    fn embedded_artifacts_have_fixed_program_arguments() {
        let unit = std::str::from_utf8(UNIT).unwrap();
        let entries: Vec<_> = unit.lines().filter_map(|l| l.split_once('=')).collect();
        assert_eq!(
            entries
                .iter()
                .filter(|(k, _)| *k == "ExecStart")
                .map(|(_, v)| *v)
                .collect::<Vec<_>>(),
            ["/usr/local/libexec/arxcovo/arxd service run"]
        );
        let dropin = std::str::from_utf8(DROPIN).unwrap();
        assert!(dropin.lines().any(|l| l == "NoNewPrivileges=yes"));
        let plist = std::str::from_utf8(PLIST).unwrap();
        let args = plist
            .split("<key>ProgramArguments</key>")
            .nth(1)
            .unwrap()
            .split("</array>")
            .next()
            .unwrap();
        let strings: Vec<_> = args
            .split("<string>")
            .skip(1)
            .map(|s| s.split("</string>").next().unwrap())
            .collect();
        assert_eq!(
            strings,
            [
                "/Library/Application Support/Arxcovo/libexec/arxd",
                "service",
                "run"
            ]
        );
        for text in [unit, dropin, plist] {
            for forbidden in ["$", "%", "/bin/sh", "cmd.exe", "powershell", "UserName"] {
                assert!(!text.contains(forbidden));
            }
        }
    }
}
