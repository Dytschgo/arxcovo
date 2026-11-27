use super::super::{Child, launch_matches};
use super::*;
use crate::cluster::consensus::activation::Intent;
use crate::service::{
    cluster_install::{INTENTS, tests::IDS, tests::request},
    config::{Account, ServiceLaunch},
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
        ] {
            let dir = fixture.path(name);
            std::fs::create_dir(&dir).unwrap();
            fixture.qualify(&dir, mode);
        }
        // The output directory belongs to the publishing caller and is
        // searchable by the daemon; the root lets the operator's control
        // group reach the control directory.
        std::fs::create_dir(fixture.path("config")).unwrap();
        fixture.chmod("config", 0o711);
        fixture.qualify(fixture.root.path(), 0o710);
        fixture
    }

    fn chmod(&self, name: &str, mode: u32) {
        std::fs::set_permissions(self.path(name), std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Publication's view: the worker daemon and the operator, whose current
    /// groups in this fixture are the fixture group.
    fn access(&self) -> Access {
        let mut access = Access::worker(self.uid, self.gid);
        access.owner = Some(Identity {
            uid: self.ids().owner_uid,
            groups: vec![self.gid],
        });
        access
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

    fn vet(&self) -> impl Fn(&Path, Option<&File>) -> Option<Vetted> + '_ {
        move |path: &Path, staged: Option<&File>| {
            vetted_objects(path, staged, self.uid, &[], &self.access())
        }
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
    let refused = write_private(
        &out,
        &bytes,
        (fixture.uid, fixture.gid),
        |path: &Path, staged: Option<&File>| {
            // The staged file is complete, private and unnamed when vetted.
            let file = staged.expect("the first check sees the staged file");
            let metadata = file.metadata().unwrap();
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_eq!(metadata.nlink(), 0);
            assert_eq!((metadata.uid(), metadata.gid()), (fixture.uid, fixture.gid));
            let mut content = vec![0; bytes.len()];
            std::os::unix::fs::FileExt::read_exact_at(file, &mut content, 0).unwrap();
            assert_eq!(content, bytes);
            seen.push(path.to_path_buf());
            None
        },
    );
    assert_eq!(refused, Err(InstallError::Layout));
    assert_eq!(seen, std::slice::from_ref(&out));
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
    let access = fixture.access();
    let result = write_private(
        &out,
        &bytes,
        (fixture.uid, fixture.gid),
        |path: &Path, staged: Option<&File>| {
            calls.set(calls.get() + 1);
            staged.and_then(|_| vetted_objects(path, staged, fixture.uid, &[], &access))
        },
    );
    assert_eq!(result, Err(InstallError::Layout));
    assert_eq!(calls.get(), 2);
    assert!(fixture.entries("config").is_empty());
}

#[test]
fn layout_checks_must_observe_the_created_file_and_qualified_directory() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    // A check that observed different objects authorizes nothing, even when
    // it reports success for the same pathname.
    let forgeries: [fn(Vetted) -> Vetted; 2] = [
        |v: Vetted| Vetted {
            file: (v.file.0, v.file.1 ^ 1),
            parent: v.parent,
        },
        |v: Vetted| Vetted {
            file: v.file,
            parent: (v.parent.0, v.parent.1 ^ 1),
        },
    ];
    let access = fixture.access();
    for forged in forgeries {
        let result = write_private(
            &out,
            &bytes,
            (fixture.uid, fixture.gid),
            |path: &Path, staged: Option<&File>| {
                vetted_objects(path, staged, fixture.uid, &[], &access).map(forged)
            },
        );
        assert_eq!(result, Err(InstallError::Layout));
        assert!(fixture.entries("config").is_empty());
    }
    // The genuine identities are exactly the staged inode, which is the
    // published file, and the qualified directory.
    let mut observed = Vec::new();
    write_private(
        &out,
        &bytes,
        (fixture.uid, fixture.gid),
        |path: &Path, staged: Option<&File>| {
            let vetted = vetted_objects(path, staged, fixture.uid, &[], &access);
            observed.push(vetted);
            vetted
        },
    )
    .unwrap();
    let file = std::fs::symlink_metadata(&out).unwrap();
    let dir = std::fs::symlink_metadata(fixture.path("config")).unwrap();
    let expected = Vetted {
        file: (file.dev(), file.ino()),
        parent: (dir.dev(), dir.ino()),
    };
    assert_eq!(observed, [Some(expected), Some(expected)]);
}

#[test]
fn every_ancestor_is_qualified_before_anything_is_created() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let never =
        |_: &Path, _: Option<&File>| -> Option<Vetted> { panic!("nothing is created or vetted") };
    // A symlink above the immediate parent is refused during the walk.
    std::fs::create_dir(fixture.path("real")).unwrap();
    fixture.qualify(&fixture.path("real"), 0o700);
    std::fs::create_dir(fixture.path("real/sub")).unwrap();
    fixture.qualify(&fixture.path("real/sub"), 0o700);
    std::os::unix::fs::symlink(fixture.path("real"), fixture.path("up")).unwrap();
    assert_eq!(
        write_private(
            &fixture.path("up/sub/cluster.json"),
            &bytes,
            (fixture.uid, fixture.gid),
            never
        ),
        Err(InstallError::Layout)
    );
    assert!(fixture.entries("real/sub").is_empty());
    // A group- or world-writable ancestor is refused before creation.
    for mode in [0o770, 0o707] {
        fixture.qualify(fixture.root.path(), mode);
        assert_eq!(
            write_private(
                &fixture.path("config/cluster.json"),
                &bytes,
                (fixture.uid, fixture.gid),
                never
            ),
            Err(InstallError::Layout)
        );
        fixture.qualify(fixture.root.path(), 0o710);
        assert!(fixture.entries("config").is_empty());
    }
    // An ancestor owned by another account is refused.
    assert_eq!(
        write_private(
            &fixture.path("config/cluster.json"),
            &bytes,
            (fixture.uid.wrapping_add(1), fixture.gid),
            never
        ),
        Err(InstallError::Layout)
    );
    assert!(fixture.entries("config").is_empty());
    // A non-directory parent and a relative path are refused.
    std::fs::write(fixture.path("plain"), b"").unwrap();
    assert_eq!(
        write_private(
            &fixture.path("plain/cluster.json"),
            &bytes,
            (fixture.uid, fixture.gid),
            never
        ),
        Err(InstallError::Layout)
    );
    assert_eq!(
        write_private(
            Path::new("relative/cluster.json"),
            &bytes,
            (fixture.uid, fixture.gid),
            never
        ),
        Err(InstallError::Argument)
    );
}

#[test]
fn output_directory_is_caller_owned_and_writable_by_nobody_else() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    let never =
        |_: &Path, _: Option<&File>| -> Option<Vetted> { panic!("nothing is staged or vetted") };
    // Group- or world-writable output directories, sticky or not, would let
    // another writer change names during publication or rollback.
    for mode in [0o731, 0o713, 0o1733, 0o1777] {
        fixture.chmod("config", mode);
        assert_eq!(
            write_private(&out, &bytes, (fixture.uid, fixture.gid), never),
            Err(InstallError::Layout),
            "{mode:o}"
        );
        assert!(fixture.entries("config").is_empty());
    }
    fixture.chmod("config", 0o711);
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } == 0 {
        // A daemon-owned output directory is refused.
        let config = fixture.path("config");
        std::os::unix::fs::chown(&config, Some(fixture.uid), Some(fixture.gid)).unwrap();
        assert_eq!(
            write_private(&out, &bytes, (fixture.uid, fixture.gid), never),
            Err(InstallError::Layout)
        );
        std::os::unix::fs::chown(&config, Some(0), Some(0)).unwrap();
        assert!(fixture.entries("config").is_empty());
    }
}

#[test]
fn a_destination_created_before_the_link_is_never_replaced() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    let access = fixture.access();
    // The name appears after the early existence check and after the staged
    // file passed its checks: linkat refuses with EEXIST.
    let result = write_private(
        &out,
        &bytes,
        (fixture.uid, fixture.gid),
        |path: &Path, staged: Option<&File>| {
            let vetted = vetted_objects(path, staged, fixture.uid, &[], &access);
            assert!(vetted.is_some());
            std::fs::write(&out, b"planted").unwrap();
            vetted
        },
    );
    assert_eq!(result, Err(InstallError::Exists));
    assert_eq!(std::fs::read(&out).unwrap(), b"planted");
    assert_eq!(fixture.entries("config"), ["cluster.json"]);
}

#[test]
fn search_permission_uses_owner_group_and_other_bits() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("d");
    std::fs::create_dir(&dir).unwrap();
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } == 0 {
        // Root bypasses the bits; give the directory an ordinary owner.
        std::os::unix::fs::chown(&dir, Some(65534), Some(65534)).unwrap();
    }
    let probe = |mode: u32| {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
        std::fs::symlink_metadata(&dir).unwrap()
    };
    let metadata = probe(0o700);
    let stranger_uid = if metadata.uid() == u32::MAX - 1 {
        u32::MAX - 2
    } else {
        u32::MAX - 1
    };
    let owner = Identity {
        uid: metadata.uid(),
        groups: vec![],
    };
    let member = Identity {
        uid: stranger_uid,
        groups: vec![metadata.gid()],
    };
    let other = Identity {
        uid: stranger_uid,
        groups: vec![],
    };
    let root_identity = Identity {
        uid: 0,
        groups: vec![],
    };
    for (mode, by_owner, by_member, by_other) in [
        (0o700, true, false, false),
        (0o710, true, true, false),
        (0o701, true, false, true),
        (0o711, true, true, true),
        // POSIX picks the owner class first, then the group class.
        (0o071, false, true, true),
        (0o601, false, false, true),
        (0o000, false, false, false),
    ] {
        let metadata = probe(mode);
        assert_eq!(owner.searches(&metadata), by_owner, "{mode:o}");
        assert_eq!(member.searches(&metadata), by_member, "{mode:o}");
        assert_eq!(other.searches(&metadata), by_other, "{mode:o}");
        assert!(root_identity.searches(&metadata));
    }
    probe(0o700);
    let file = root.path().join("f");
    std::fs::write(&file, b"").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o711)).unwrap();
    let file = std::fs::symlink_metadata(&file).unwrap();
    assert!(!owner.searches(&file));
    assert!(!root_identity.searches(&file));
}

#[test]
fn runtime_identities_must_search_every_directory_they_traverse() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    // The operator cannot reach the control directory through a 0700
    // ancestor that root publication itself could open.
    fixture.qualify(fixture.root.path(), 0o700);
    assert_eq!(
        write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()),
        Err(InstallError::Layout)
    );
    assert!(fixture.entries("config").is_empty());
    fixture.qualify(fixture.root.path(), 0o710);
    write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()).unwrap();
    let check = || Settings::vetted_layout(&out, None, fixture.uid, &[], None).unwrap();
    let (settings, _, pass) = check();
    assert!(fixture.access().permits(&pass, &settings, &out));
    // The configuration path: an identity that reaches the fixture root
    // through its group cannot pass a 0700 output directory it does not own.
    let stranger_uid = if fixture.uid == u32::MAX - 1 {
        u32::MAX - 2
    } else {
        u32::MAX - 1
    };
    let stranger = Identity {
        uid: stranger_uid,
        groups: vec![],
    };
    let member = Identity {
        uid: stranger_uid,
        groups: vec![fixture.gid],
    };
    assert!(chain_reachable(&pass, &out, false, &member));
    assert!(!chain_reachable(&pass, &out, false, &stranger));
    fixture.chmod("config", 0o700);
    let (settings, _, pass) = check();
    assert!(!chain_reachable(&pass, &out, false, &member));
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } == 0 {
        // A root-owned 0700 output directory: the daemon cannot open the file.
        let (settings, _, pass) = check();
        assert!(!fixture.access().permits(&pass, &settings, &out));
    }
    // Private directories and the control directory count for the daemon,
    // and the control directory for the operator.
    let blocked = Access {
        daemon: stranger.clone(),
        owner: fixture.access().owner,
    };
    assert!(!blocked.permits(&pass, &settings, &out));
    let blocked = Access {
        daemon: fixture.access().daemon,
        owner: Some(stranger),
    };
    fixture.chmod("config", 0o711);
    let (settings, _, pass) = check();
    assert!(!blocked.permits(&pass, &settings, &out));
}

/// An extended POSIX ACL xattr matching `mode`, plus a named-user entry that
/// grants `named` nothing. Setting it keeps the mode bits unchanged.
fn extended_acl(mode: u32, named: u32) -> Vec<u8> {
    let (owner, group, other) = ((mode >> 6) & 7, (mode >> 3) & 7, mode & 7);
    let mut value = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01u16, owner, u32::MAX),
        (0x02, 0, named),
        (0x04, group, u32::MAX),
        (0x10, group, u32::MAX),
        (0x20, other, u32::MAX),
    ] {
        value.extend(tag.to_le_bytes());
        value.extend((perm as u16).to_le_bytes());
        value.extend(id.to_le_bytes());
    }
    value
}

/// Sets or removes an xattr; `Ok(false)` when the filesystem has no ACLs.
fn set_acl(path: &Path, attribute: &std::ffi::CStr, value: Option<&[u8]>) -> Result<bool, i32> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: path and attribute are NUL terminated; value is live.
    let rc = unsafe {
        match value {
            Some(value) => libc::setxattr(
                path.as_ptr(),
                attribute.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            ),
            None => libc::removexattr(path.as_ptr(), attribute.as_ptr()),
        }
    };
    match rc {
        0 => Ok(true),
        _ => match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EOPNOTSUPP) => Ok(false),
            other => Err(other.unwrap_or(-1)),
        },
    }
}

#[test]
fn extended_acls_on_config_and_private_ancestors_are_refused() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()).unwrap();
    let other = fixture.path("config/other.json");
    let check = || Settings::vetted_layout(&out, None, fixture.uid, &[], None).unwrap();
    let worker = Access::worker(fixture.uid, fixture.gid);
    let (settings, _, pass) = check();
    assert!(worker.permits(&pass, &settings, &out));
    let named = if fixture.uid == u32::MAX - 1 {
        u32::MAX - 2
    } else {
        u32::MAX - 1
    };
    // The output directory (config ancestor), the custody parent and the
    // control directory (private and owner-socket chains).
    for (name, mode) in [("config", 0o711), ("pki", 0o700), ("control", 0o2710)] {
        let path = fixture.path(name);
        let acl = extended_acl(mode, named);
        match set_acl(&path, c"system.posix_acl_access", Some(&acl)) {
            Ok(true) => {}
            // The test filesystem has no POSIX ACL support; nothing to observe.
            Ok(false) => return,
            Err(errno) => panic!("setxattr {name}: {errno}"),
        }
        assert_eq!(
            std::fs::symlink_metadata(&path).unwrap().mode() & 0o7777,
            mode,
            "{name}"
        );
        // `Child::qualify`'s worker check refuses the layout.
        let (settings, _, pass) = check();
        assert!(!worker.permits(&pass, &settings, &out), "{name}");
        assert!(!fixture.access().permits(&pass, &settings, &out), "{name}");
        // Publication refuses and names nothing.
        assert_eq!(
            write_private(&other, &bytes, (fixture.uid, fixture.gid), fixture.vet()),
            Err(InstallError::Layout),
            "{name}"
        );
        assert_eq!(fixture.entries("config"), ["cluster.json"]);
        assert_eq!(set_acl(&path, c"system.posix_acl_access", None), Ok(true));
        let (settings, _, pass) = check();
        assert!(worker.permits(&pass, &settings, &out), "{name}");
    }
    // An extended ACL on the configuration file itself fails the worker check.
    assert_eq!(
        set_acl(
            &out,
            c"system.posix_acl_access",
            Some(&extended_acl(0o600, named))
        ),
        Ok(true)
    );
    let (settings, _, pass) = check();
    assert!(!worker.permits(&pass, &settings, &out));
    assert_eq!(set_acl(&out, c"system.posix_acl_access", None), Ok(true));
    // A default ACL on a private directory is refused too.
    let state = fixture.path("state");
    assert_eq!(
        set_acl(
            &state,
            c"system.posix_acl_default",
            Some(&extended_acl(0o700, named))
        ),
        Ok(true)
    );
    let (settings, _, pass) = check();
    assert!(!worker.permits(&pass, &settings, &out));
}

#[test]
fn daemon_traversal_ignores_supplementary_groups() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let out = fixture.path("config/cluster.json");
    // Publication and `Child::qualify` check the daemon as the native worker
    // runs: its UID and primary GID only.
    let worker = Access::worker(fixture.uid, fixture.gid);
    assert_eq!(worker.daemon, Identity::worker(fixture.uid, fixture.gid));
    assert_eq!(worker.daemon.groups, [fixture.gid]);
    assert!(worker.owner.is_none());
    assert_eq!(fixture.access().daemon, worker.daemon);
    write_private(&out, &bytes, (fixture.uid, fixture.gid), fixture.vet()).unwrap();
    let (settings, _, pass) = Settings::vetted_layout(&out, None, fixture.uid, &[], None).unwrap();
    // The fixture root (0710) is searchable only through its group. An
    // identity holding that group only as a supplementary group reaches the
    // file and the control directory; the same identity as a worker does not.
    let uid = if fixture.uid == u32::MAX - 1 {
        u32::MAX - 2
    } else {
        u32::MAX - 1
    };
    let primary = if fixture.gid == u32::MAX - 1 {
        u32::MAX - 2
    } else {
        u32::MAX - 1
    };
    let supplementary = Identity {
        uid,
        groups: vec![primary, fixture.gid],
    };
    let worker_view = Identity::worker(uid, primary);
    let control = settings.owner_socket.parent().unwrap();
    assert!(chain_reachable(&pass, &out, false, &supplementary));
    assert!(chain_reachable(&pass, control, true, &supplementary));
    assert!(!chain_reachable(&pass, &out, false, &worker_view));
    assert!(!chain_reachable(&pass, control, true, &worker_view));
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } == 0 {
        // Full flow as root: the root directory belongs to another account and
        // a group the daemon holds only as a supplementary group.
        let group = primary;
        std::os::unix::fs::chown(fixture.root.path(), Some(0), Some(group)).unwrap();
        let mut supplementary = Access::worker(fixture.uid, fixture.gid);
        supplementary.daemon.groups.push(group);
        let (settings, _, pass) =
            Settings::vetted_layout(&out, None, fixture.uid, &[], None).unwrap();
        assert!(supplementary.permits(&pass, &settings, &out));
        assert!(!worker.permits(&pass, &settings, &out));
        assert_eq!(
            write_private(
                &fixture.path("config/other.json"),
                &bytes,
                (fixture.uid, fixture.gid),
                fixture.vet()
            ),
            Err(InstallError::Layout)
        );
        assert_eq!(fixture.entries("config"), ["cluster.json"]);
        std::os::unix::fs::chown(fixture.root.path(), Some(fixture.uid), Some(fixture.gid))
            .unwrap();
    }
}

/// A minimal POSIX ACL xattr: version 2 with user, group and other entries.
fn minimal_acl() -> Vec<u8> {
    let mut value = 2u32.to_le_bytes().to_vec();
    for (tag, perm) in [(0x01u16, 7u16), (0x04, 5), (0x20, 0)] {
        value.extend(tag.to_le_bytes());
        value.extend(perm.to_le_bytes());
        value.extend(u32::MAX.to_le_bytes());
    }
    value
}

#[test]
fn owner_socket_route_refuses_posix_acls_before_publication() {
    let fixture = Fixture::new();
    let r = fixture.request(Role::Founder);
    let bytes = install::document(&r, &fixture.ids(), Some([9; 16])).unwrap();
    let settings = parity(&bytes).unwrap();
    assert_eq!(socket_route(&settings), Ok(()));
    let control = std::ffi::CString::new(fixture.text("control")).unwrap();
    let acl = minimal_acl();
    // SAFETY: the path and attribute name are NUL terminated and the value
    // buffer is live for the call.
    let rc = unsafe {
        libc::setxattr(
            control.as_ptr(),
            c"system.posix_acl_default".as_ptr(),
            acl.as_ptr().cast(),
            acl.len(),
            0,
        )
    };
    if rc != 0 {
        // The test filesystem has no POSIX ACL support; nothing to observe.
        let error = std::io::Error::last_os_error().raw_os_error();
        assert_eq!(error, Some(libc::EOPNOTSUPP), "setxattr");
        return;
    }
    assert_eq!(socket_route(&settings), Err(InstallError::Layout));
    // A wrong control group or mode is refused too.
    // SAFETY: as above; removing the attribute restores the clean layout.
    assert_eq!(
        unsafe { libc::removexattr(control.as_ptr(), c"system.posix_acl_default".as_ptr()) },
        0
    );
    assert_eq!(socket_route(&settings), Ok(()));
    fixture.qualify(&fixture.path("control"), 0o2750);
    assert_eq!(socket_route(&settings), Err(InstallError::Layout));
}

#[test]
fn role_and_intent_pairs_match_the_worker_rule() {
    let founder =
        parity(&install::document(&request(Role::Founder), &IDS, Some([1; 16])).unwrap()).unwrap();
    let joiner = parity(&install::document(&request(Role::Joiner), &IDS, None).unwrap()).unwrap();
    for (intent, founder_ok, joiner_ok) in [
        ("create-founder", true, false),
        ("recover-founder", true, false),
        ("join", false, true),
        ("recover-joiner", false, true),
    ] {
        let parsed = Intent::parse(intent).unwrap();
        assert_eq!(founder.compatible(parsed), founder_ok, "{intent}");
        assert_eq!(joiner.compatible(parsed), joiner_ok, "{intent}");
        // `service configure` uses the same rule through `Child::qualify`.
        let launch = |gid| ServiceLaunch {
            config: "/etc/arxcovo-cluster/cluster.json".into(),
            intent: intent.into(),
            daemon_uid: IDS.daemon_uid,
            daemon_gid: gid,
        };
        assert_eq!(
            launch_matches(&founder, &launch(IDS.daemon_gid)),
            founder_ok
        );
        assert_eq!(launch_matches(&joiner, &launch(IDS.daemon_gid)), joiner_ok);
        assert!(!launch_matches(&founder, &launch(IDS.daemon_gid + 1)));
        assert!(!launch_matches(&joiner, &launch(IDS.daemon_gid + 1)));
    }
    let mut unknown = ServiceLaunch {
        config: "/etc/arxcovo-cluster/cluster.json".into(),
        intent: "founder".into(),
        daemon_uid: IDS.daemon_uid,
        daemon_gid: IDS.daemon_gid,
    };
    assert!(!launch_matches(&founder, &unknown));
    unknown.intent = "create-founder".into();
    assert!(launch_matches(&founder, &unknown));
    // A zero founder cluster ID never fits any intent.
    let mut zero = founder;
    zero.founder_cluster = Some([0; 16]);
    for intent in INTENTS {
        assert!(!zero.compatible(Intent::parse(intent).unwrap()));
    }
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
