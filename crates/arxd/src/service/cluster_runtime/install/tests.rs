use super::super::Child;
use super::*;
use crate::cluster::consensus::activation::Intent;
use crate::service::{
    cluster_install::{INTENTS, tests::IDS, tests::request},
    config::Account,
    layout::{Error, Platform},
};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

struct Fixture {
    root: tempfile::TempDir,
    uid: u32,
    gid: u32,
}

impl Fixture {
    /// The `private_fixture` layout from `cluster_runtime::tests`, without a
    /// configuration file. Root tests use an unprivileged daemon identity.
    fn new() -> Self {
        // SAFETY: scalar identity getters take no pointers.
        let (euid, egid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let (uid, gid) = if euid == 0 {
            (65534, 65534)
        } else {
            (euid, egid)
        };
        let root = tempfile::tempdir().unwrap();
        let fixture = Self { root, uid, gid };
        for (name, mode) in [
            ("workspace", 0o700),
            ("state", 0o700),
            ("credentials", 0o700),
            ("wrap", 0o700),
            ("pki", 0o700),
            ("pki/custody", 0o700),
            ("history", 0o700),
            ("control", 0o2710),
            ("config", 0o700),
        ] {
            let dir = fixture.path(name);
            std::fs::create_dir(&dir).unwrap();
            fixture.qualify(&dir, mode);
        }
        fixture.qualify(fixture.root.path(), 0o700);
        fixture
    }

    fn qualify(&self, path: &Path, mode: u32) {
        // SAFETY: geteuid takes no pointers.
        if unsafe { libc::geteuid() } == 0 {
            std::os::unix::fs::chown(path, Some(self.uid), Some(self.gid)).unwrap();
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn text(&self, name: &str) -> String {
        self.path(name).to_str().unwrap().into()
    }

    fn request(&self, role: Role) -> Request {
        let mut r = request(role);
        r.out = self.text("config/cluster.json");
        r.workspace = self.text("workspace");
        r.state = self.text("state");
        r.credentials = self.text("credentials");
        r.wrap = self.text("wrap");
        r.custody = self.text("pki/custody");
        r.history = self.text("history");
        r.owner_socket = self.text("control/owner");
        r
    }

    fn ids(&self) -> Ids {
        let others = [11, 12, 13, 14]
            .into_iter()
            .filter(|id| *id != self.uid)
            .collect::<Vec<_>>();
        Ids {
            owner_uid: others[0],
            owner_gid: self.gid,
            daemon_uid: self.uid,
            daemon_gid: self.gid,
            agent_uid: others[1],
            build_uid: others[2],
        }
    }

    fn vet(&self) -> impl Fn(&Path) -> bool + '_ {
        move |path: &Path| Settings::vetted(path, self.uid, &[], None).is_ok()
    }

    fn entries(&self, name: &str) -> Vec<String> {
        let mut names = std::fs::read_dir(self.path(name))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

#[test]
fn written_founder_and_joiner_files_round_trip_through_the_runtime_loader() {
    for (role, founder) in [(Role::Founder, Some([9; 16])), (Role::Joiner, None)] {
        let fixture = Fixture::new();
        let r = fixture.request(role);
        let ids = fixture.ids();
        let bytes = install::document(&r, &ids, founder).unwrap();
        let parsed = parity(&bytes).unwrap();
        assert_eq!(parsed.founder_cluster, founder);
        let out = fixture.path("config/cluster.json");
        write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        let metadata = std::fs::symlink_metadata(&out).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!((metadata.uid(), metadata.gid()), (fixture.uid, fixture.gid));
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(fixture.entries("config"), ["cluster.json"]);
        let (settings, _) = Settings::vetted(&out, fixture.uid, &[], None).unwrap();
        assert_eq!(settings.workspace, fixture.path("workspace"));
        assert_eq!(settings.state, fixture.path("state"));
        assert_eq!(settings.credentials, fixture.path("credentials"));
        assert_eq!(settings.wrap, fixture.path("wrap"));
        assert_eq!(settings.custody, fixture.path("pki/custody"));
        assert_eq!(settings.history, fixture.path("history"));
        assert_eq!(settings.owner_socket, fixture.path("control/owner"));
        assert_eq!(settings.peer, r.peer);
        assert_eq!(settings.enrollment, r.enrollment);
        assert_eq!(settings.issuer_peer, r.issuer_peer);
        assert_eq!(settings.founder_cluster, founder);
        assert!(settings.executor_socket.is_none());
        assert!(settings.delegated_build.is_none());
        assert_eq!(
            [
                settings.owner_uid,
                settings.owner_gid,
                settings.daemon_uid,
                settings.daemon_gid,
                settings.agent_uid,
                settings.build_uid,
            ],
            [
                ids.owner_uid,
                ids.owner_gid,
                ids.daemon_uid,
                ids.daemon_gid,
                ids.agent_uid,
                ids.build_uid,
            ]
        );
        // The installed file never changes once written.
        let other = install::document(&r, &ids, founder.map(|_| [8; 16])).unwrap();
        assert_eq!(
            write_private(&out, &other, (fixture.uid, fixture.gid), fixture.vet()),
            Err(InstallError::Exists)
        );
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert_eq!(fixture.entries("config"), ["cluster.json"]);
    }
}

#[test]
fn refused_layouts_publish_nothing() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    let mut seen = Vec::new();
    let refused = write_private(&out, &bytes, (fixture.uid, fixture.gid), |path: &Path| {
        // The temporary file is complete and private when it is vetted.
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        seen.push(path.to_path_buf());
        false
    });
    assert_eq!(refused, Err(InstallError::Layout));
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].parent(), out.parent());
    assert!(fixture.entries("config").is_empty());
    // A group-readable private directory fails the runtime loader.
    fixture.qualify(&fixture.path("state"), 0o750);
    assert_eq!(
        write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()),
        Err(InstallError::Layout)
    );
    assert!(fixture.entries("config").is_empty());
    fixture.qualify(&fixture.path("state"), 0o700);
    // A socket directory without the setgid control mode is refused.
    fixture.qualify(&fixture.path("control"), 0o710);
    assert_eq!(
        write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()),
        Err(InstallError::Layout)
    );
    assert!(fixture.entries("config").is_empty());
    fixture.qualify(&fixture.path("control"), 0o2710);
    // A missing private directory is refused.
    std::fs::remove_dir(fixture.path("history")).unwrap();
    assert_eq!(
        write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()),
        Err(InstallError::Layout)
    );
    assert!(fixture.entries("config").is_empty());
}

#[test]
fn symlinked_targets_and_parents_are_refused() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    // A dangling symlink at the final name is an existing entry.
    let out = fixture.path("config/cluster.json");
    std::os::unix::fs::symlink(fixture.path("elsewhere.json"), &out).unwrap();
    assert_eq!(
        write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()),
        Err(InstallError::Exists)
    );
    assert!(!fixture.path("elsewhere.json").exists());
    // A symlinked parent directory is never opened.
    std::os::unix::fs::symlink(fixture.path("config"), fixture.path("alias")).unwrap();
    assert_eq!(
        write_private(
            &fixture.path("alias/other.json"),
            &bytes,
            (fixture.uid, fixture.gid),
            fixture.vet()
        ),
        Err(InstallError::Layout)
    );
    assert_eq!(fixture.entries("config"), ["cluster.json"]);
}

#[test]
fn a_published_file_that_fails_its_final_vet_is_removed() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    let calls = std::cell::Cell::new(0);
    let result = write_private(&out, &bytes, (fixture.uid, fixture.gid), |path: &Path| {
        calls.set(calls.get() + 1);
        path != out
    });
    assert_eq!(result, Err(InstallError::Layout));
    assert_eq!(calls.get(), 2);
    assert!(fixture.entries("config").is_empty());
}

#[test]
fn schema_endpoint_socket_and_intent_rules_match_the_runtime() {
    let bytes = install::document(&request(Role::Joiner), &IDS, None).unwrap();
    assert!(parity(&bytes).is_ok());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["unexpected"] = 1.into();
    assert_eq!(
        parity(&serde_json::to_vec(&value).unwrap()).err(),
        Some(InstallError::Argument)
    );
    for socket in [
        "/var/lib/arxcovo-cluster/control/owner.sock",
        "/owner.sock",
        "/a//b",
        "/a/b c",
        "/a/./b",
        "/a/b/",
        &format!("/a/{}", "b".repeat(97)),
        &format!("/a/{}", "b".repeat(98)),
    ] {
        assert_eq!(
            install::owner_socket(socket),
            cluster_owner::address(Path::new(socket)),
            "{socket}"
        );
    }
    for address in [
        "192.168.1.20:7400",
        "10.1.2.3:1",
        "172.31.0.1:65535",
        "100.64.0.1:7400",
        "100.128.0.1:7400",
        "[fd00::1]:7400",
        "[fe80::1]:7400",
        "[::ffff:192.168.1.20]:7400",
        "8.8.8.8:7400",
        "127.0.0.1:7400",
        "192.168.1.20:0",
        "192.168.001.20:7400",
        "255.255.255.255:7400",
    ] {
        assert_eq!(
            install::endpoint(address),
            peer_endpoint(address),
            "{address}"
        );
    }
    for intent in INTENTS {
        assert!(Intent::parse(intent).is_some());
    }
    for intent in ["", "founder", "Join", "create_founder", "recover"] {
        assert!(Intent::parse(intent).is_none());
        assert!(!INTENTS.contains(&intent));
    }
}

#[test]
fn routes_mirror_the_join_issuer_rule() {
    let parsed = |role| {
        parity(
            &install::document(
                &request(role),
                &IDS,
                (role == Role::Founder).then_some([1; 16]),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let route = |network: &str, prefix| Route {
        network: network.parse().unwrap(),
        prefix,
    };
    let peer: IpAddr = "192.168.1.20".parse().unwrap();
    let lan = [(peer, route("192.168.1.0", 24))];
    assert!(reachable(&parsed(Role::Founder), &lan));
    assert!(reachable(&parsed(Role::Joiner), &lan));
    // The peer address must be local.
    let foreign = [("192.168.1.21".parse().unwrap(), route("192.168.1.0", 24))];
    assert!(!reachable(&parsed(Role::Founder), &foreign));
    assert!(!reachable(&parsed(Role::Founder), &[]));
    // A host-only (/32) interface, such as a Tailscale address, cannot reach
    // a joiner's issuer; a founder does not need an issuer route.
    let host = [(peer, route("192.168.1.20", 32))];
    assert!(reachable(&parsed(Role::Founder), &host));
    assert!(!reachable(&parsed(Role::Joiner), &host));
    let other = [(peer, route("192.168.1.0", 30))];
    assert!(!reachable(&parsed(Role::Joiner), &other));
}

struct Accounts;
impl Resolver for Accounts {
    fn account(&self, value: &str) -> Result<Account, Error> {
        let (identity, gid, administrative) = match value {
            "arxd-cluster" => ("1100", 1100, false),
            "arxd-operator" => ("1101", 1101, true),
            "owner-login" => ("1000", 1000, true),
            "arxcovo" => ("1102", 1102, false),
            "admin-build" => ("1103", 1103, true),
            "group-only" => {
                return Ok(Account {
                    identity: "1104".into(),
                    gid: Some(1104),
                    kind: AccountKind::Group,
                    administrative: false,
                });
            }
            "no-gid" => ("1105", 0, false),
            "named" => ("not-numeric", 1106, false),
            _ => return Err(Error::AccountMissing),
        };
        Ok(Account {
            identity: identity.into(),
            gid: (gid != 0).then_some(gid),
            kind: AccountKind::User,
            administrative,
        })
    }
    fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
        panic!("cluster-config never observes a service workspace")
    }
}

#[test]
fn accounts_resolve_to_the_fixed_daemon_and_named_roles() {
    let control = |name: &str| match name {
        "arxd-control" => Ok(1200),
        _ => Err(InstallError::Account),
    };
    let r = request(Role::Founder);
    assert_eq!(resolve(&r, &Accounts, control), Ok(IDS));
    type Field = fn(&mut Request) -> &mut String;
    let cases: [(Field, &str); 6] = [
        (|r| &mut r.build_account, "admin-build"),
        (|r| &mut r.build_account, "missing"),
        (|r| &mut r.operator_account, "group-only"),
        (|r| &mut r.operator_account, "no-gid"),
        (|r| &mut r.agent_account, "named"),
        (|r| &mut r.control_group, "missing-group"),
    ];
    for (field, value) in cases {
        let mut r = request(Role::Founder);
        *field(&mut r) = value.into();
        assert_eq!(
            resolve(&r, &Accounts, control),
            Err(InstallError::Account),
            "{value}"
        );
    }
    struct AdminDaemon;
    impl Resolver for AdminDaemon {
        fn account(&self, value: &str) -> Result<Account, Error> {
            let mut account = Accounts.account(value)?;
            account.administrative |= value == "arxd-cluster";
            Ok(account)
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            panic!("no workspace observation")
        }
    }
    assert_eq!(
        resolve(&r, &AdminDaemon, control),
        Err(InstallError::Account)
    );
}

#[test]
fn group_lookup_accepts_names_and_canonical_numbers() {
    assert_eq!(group("root"), Ok(0));
    assert_eq!(group("0"), Ok(0));
    assert_eq!(group("00"), Err(InstallError::Account));
    assert_eq!(
        group("arxd-no-such-group-for-tests"),
        Err(InstallError::Account)
    );
    assert_eq!(group("a\0b"), Err(InstallError::Account));
}

#[test]
fn founder_ids_are_random_and_nonzero() {
    let (a, b) = (founder_cluster().unwrap(), founder_cluster().unwrap());
    assert_ne!(a, [0; 16]);
    assert_ne!(a, b);
}

#[test]
fn publish_checks_arguments_then_requires_full_root() {
    let layout = Layout::for_test(Platform::Linux);
    let mut bad = request(Role::Founder);
    bad.owner_socket = "/owner.sock".into();
    assert_eq!(publish(&bad, &layout), Err(InstallError::Argument));
    // SAFETY: scalar identity getters take no pointers.
    if unsafe { libc::getuid() | libc::geteuid() | libc::getgid() | libc::getegid() } != 0 {
        assert_eq!(
            publish(&request(Role::Founder), &layout),
            Err(InstallError::OwnerRequired)
        );
    }
}

#[test]
fn configure_preflight_runs_the_start_time_launch_checks() {
    use crate::service::config::tests::{FakeResolver, fixture, parse};
    let layout = Layout::for_test(Platform::Linux);
    let mut value = fixture();
    value["cluster"] = serde_json::json!({
        "config": "/etc/arxcovo-cluster/cluster.json",
        "intent": "join",
        "daemon_uid": 1100,
        "daemon_gid": 1100,
    });
    let service = parse(value, &FakeResolver::default()).ok().unwrap();
    let launch = service.cluster().unwrap();
    let mut invalid = launch.clone();
    invalid.intent = "founder".into();
    assert_eq!(
        Child::preflight(&invalid, &layout, &service).err(),
        Some(crate::service::ExitCode::Config)
    );
    let mut relative = launch.clone();
    relative.config = "cluster.json".into();
    assert_eq!(
        Child::preflight(&relative, &layout, &service).err(),
        Some(crate::service::ExitCode::Config)
    );
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(
            Child::preflight(launch, &layout, &service).err(),
            Some(crate::service::ExitCode::Permissions)
        );
    } else {
        // Root still needs the real arxd-cluster account and vetted file.
        assert!(Child::preflight(launch, &layout, &service).is_err());
    }
}
