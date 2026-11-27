//! Platform-independent schema tests; Linux adds runtime-loader parity in
//! `cluster_runtime::install::tests`.
use super::*;

pub(crate) fn request(role: Role) -> Request {
    Request {
        out: "/etc/arxcovo-cluster/cluster.json".into(),
        role,
        workspace: "/var/lib/arxcovo-cluster/workspace".into(),
        state: "/var/lib/arxcovo-cluster/state".into(),
        credentials: "/var/lib/arxcovo-cluster/credentials".into(),
        wrap: "/var/lib/arxcovo-cluster/wrap".into(),
        custody: "/var/lib/arxcovo-cluster/pki/custody".into(),
        history: "/var/lib/arxcovo-cluster/history".into(),
        owner_socket: "/var/lib/arxcovo-cluster/control/owner.sock".into(),
        peer: "192.168.1.20:7400".into(),
        enrollment: "192.168.1.20:7401".into(),
        issuer_peer: (role == Role::Joiner).then(|| "192.168.1.10:7400".into()),
        operator_account: "arxd-operator".into(),
        control_group: "arxd-control".into(),
        agent_account: "owner-login".into(),
        build_account: "arxcovo".into(),
    }
}

pub(crate) const IDS: Ids = Ids {
    owner_uid: 1101,
    owner_gid: 1200,
    daemon_uid: 1100,
    daemon_gid: 1100,
    agent_uid: 1000,
    build_uid: 1102,
};

#[test]
fn founder_and_joiner_documents_use_the_exact_runtime_schema() {
    let founder = document(&request(Role::Founder), &IDS, Some([7; 16])).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&founder).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "workspace": "/var/lib/arxcovo-cluster/workspace",
            "state": "/var/lib/arxcovo-cluster/state",
            "credentials": "/var/lib/arxcovo-cluster/credentials",
            "wrap": "/var/lib/arxcovo-cluster/wrap",
            "custody": "/var/lib/arxcovo-cluster/pki/custody",
            "history": "/var/lib/arxcovo-cluster/history",
            "owner_socket": "/var/lib/arxcovo-cluster/control/owner.sock",
            "peer": "192.168.1.20:7400",
            "enrollment": "192.168.1.20:7401",
            "founder_cluster": vec![7; 16],
            "owner_uid": 1101,
            "daemon_uid": 1100,
            "daemon_gid": 1100,
            "agent_uid": 1000,
            "build_uid": 1102,
            "owner_gid": 1200,
        })
    );
    let joiner = document(&request(Role::Joiner), &IDS, None).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&joiner).unwrap();
    assert_eq!(value["issuer_peer"], "192.168.1.10:7400");
    assert!(value.get("founder_cluster").is_none());
    // Executor and delegated-build settings are never written.
    for document in [&founder, &joiner] {
        let value: serde_json::Value = serde_json::from_slice(document).unwrap();
        assert!(value.get("executor_socket").is_none());
        assert!(value.get("delegated_build").is_none());
        assert!(document.len() <= MAX_INSTALLATION_BYTES);
    }
    // Serialization is deterministic for identical input.
    assert_eq!(
        founder,
        document(&request(Role::Founder), &IDS, Some([7; 16])).unwrap()
    );
}

#[test]
fn role_binds_issuer_and_founder_cluster() {
    let mut founder = request(Role::Founder);
    assert_eq!(document(&founder, &IDS, None), Err(InstallError::Argument));
    assert_eq!(
        document(&founder, &IDS, Some([0; 16])),
        Err(InstallError::Argument)
    );
    founder.issuer_peer = Some("192.168.1.10:7400".into());
    assert_eq!(check(&founder), Err(InstallError::Argument));
    let mut joiner = request(Role::Joiner);
    assert_eq!(
        document(&joiner, &IDS, Some([7; 16])),
        Err(InstallError::Argument)
    );
    joiner.issuer_peer = None;
    assert_eq!(check(&joiner), Err(InstallError::Argument));
    for issuer in ["192.168.1.20:7400", "192.168.1.20:7401", "127.0.0.1:7400"] {
        let mut joiner = request(Role::Joiner);
        joiner.issuer_peer = Some(issuer.into());
        assert_eq!(check(&joiner), Err(InstallError::Argument), "{issuer}");
    }
}

#[test]
fn owner_socket_rule_matches_the_owner_endpoint_policy() {
    for good in [
        "/var/lib/arxcovo-cluster/control/owner.sock",
        "/a/b",
        &format!("/a/{}", "b".repeat(97)),
        "/run/A-z_0.9/s",
    ] {
        assert!(owner_socket(good), "{good}");
    }
    for bad in [
        "/owner.sock",
        "relative/socket",
        "/a//b",
        "/a/./b",
        "/a/../b",
        "/a/b/",
        "/a/b c",
        "/a/b\u{e9}",
        "/a/b:c",
        "/a/b\nc",
        &format!("/a/{}", "b".repeat(98)),
        "",
        "/",
    ] {
        assert!(!owner_socket(bad), "{bad:?}");
        let mut r = request(Role::Founder);
        r.owner_socket = bad.into();
        assert_eq!(check(&r), Err(InstallError::Argument), "{bad:?}");
    }
}

#[test]
fn paths_are_absolute_canonical_and_pairwise_disjoint() {
    for bad in [
        "relative",
        "/",
        "/a/",
        "/a//b",
        "/a/./b",
        "/a/../b",
        "/a/b\0",
        "/a/\u{7}",
        "C:\\cluster",
        r"\\?\C:\cluster",
    ] {
        assert!(!posix_path(bad), "{bad:?}");
    }
    assert!(!posix_path(&format!("/{}", "a".repeat(256))));
    assert!(!posix_path(&"/a".repeat(2048)));
    assert!(posix_path(&format!("/{}", "a".repeat(255))));
    type Field = fn(&mut Request) -> &mut String;
    let fields: [Field; 7] = [
        |r| &mut r.out,
        |r| &mut r.workspace,
        |r| &mut r.state,
        |r| &mut r.credentials,
        |r| &mut r.wrap,
        |r| &mut r.custody,
        |r| &mut r.history,
    ];
    for field in fields {
        for bad in ["relative/dir", "/var/lib/../etc", "/var/lib/x/"] {
            let mut r = request(Role::Founder);
            *field(&mut r) = bad.into();
            assert_eq!(check(&r), Err(InstallError::Argument), "{bad}");
        }
    }
    // A file or directory directly under `/` has no private parent.
    for (field, value) in [(fields[0], "/cluster.json"), (fields[5], "/custody")] {
        let mut r = request(Role::Founder);
        *field(&mut r) = value.into();
        assert_eq!(check(&r), Err(InstallError::Argument), "{value}");
    }
    // Every pair overlaps once: equal, nested, or nested under the socket dir.
    let paths = [
        "/var/lib/arxcovo-cluster/workspace",
        "/var/lib/arxcovo-cluster/state",
        "/var/lib/arxcovo-cluster/credentials",
        "/var/lib/arxcovo-cluster/wrap",
        "/var/lib/arxcovo-cluster/pki/custody",
        "/var/lib/arxcovo-cluster/history",
    ];
    for (i, field) in fields.iter().enumerate().skip(1) {
        for other in paths.iter().filter(|p| **p != paths[i - 1]) {
            for value in [other.to_string(), format!("{other}/nested")] {
                let mut r = request(Role::Founder);
                *field(&mut r) = value.clone();
                assert_eq!(check(&r), Err(InstallError::Argument), "{value}");
            }
        }
        let mut r = request(Role::Founder);
        *field(&mut r) = "/var/lib/arxcovo-cluster/control".into();
        assert_eq!(check(&r), Err(InstallError::Argument));
        let mut r = request(Role::Founder);
        *field(&mut r) = "/var/lib/arxcovo-cluster/control/inner".into();
        assert_eq!(check(&r), Err(InstallError::Argument));
    }
    let mut r = request(Role::Founder);
    r.out = "/var/lib/arxcovo-cluster/state/cluster.json".into();
    assert_eq!(check(&r), Err(InstallError::Argument));
    // Shared name prefixes are not containment.
    let mut r = request(Role::Founder);
    r.state = "/var/lib/arxcovo-cluster/workspace-state".into();
    assert_eq!(check(&r), Ok(()));
    assert!(!overlaps("/a/bc", "/a/b"));
    assert!(overlaps("/a/b/c", "/a/b"));
    assert!(overlaps("/a/b", "/a/b"));
}

#[test]
fn endpoints_are_canonical_private_and_paired() {
    for good in [
        "10.0.0.1:1",
        "172.16.5.4:7400",
        "192.168.1.20:65535",
        "100.64.0.1:7400",
        "100.127.255.254:7400",
        "[fd00::1]:7400",
    ] {
        assert!(endpoint(good).is_some(), "{good}");
    }
    for bad in [
        "192.168.1.20:0",
        "192.168.1.20",
        "192.168.001.20:7400",
        " 192.168.1.20:7400",
        "8.8.8.8:7400",
        "100.128.0.1:7400",
        "127.0.0.1:7400",
        "0.0.0.0:7400",
        "[::1]:7400",
        "[2001:db8::1]:7400",
        "[fe80::1]:7400",
        "[fd00::1%2]:7400",
        "[::ffff:192.168.1.20]:7400",
        "host.local:7400",
        &format!("{}:1", "1".repeat(200)),
    ] {
        assert!(endpoint(bad).is_none(), "{bad}");
    }
    for (peer, enrollment) in [
        ("192.168.1.20:7400", "192.168.1.21:7401"),
        ("192.168.1.20:7400", "192.168.1.20:7400"),
        ("192.168.1.20:7400", "[fd00::1]:7401"),
    ] {
        let mut r = request(Role::Founder);
        r.peer = peer.into();
        r.enrollment = enrollment.into();
        assert_eq!(
            check(&r),
            Err(InstallError::Argument),
            "{peer} {enrollment}"
        );
    }
}

#[test]
fn identities_are_nonzero_distinct_and_bounded() {
    let r = request(Role::Founder);
    assert!(document(&r, &IDS, Some([1; 16])).is_ok());
    type Field = fn(&mut Ids) -> &mut u32;
    let fields: [Field; 6] = [
        |i| &mut i.owner_uid,
        |i| &mut i.owner_gid,
        |i| &mut i.daemon_uid,
        |i| &mut i.daemon_gid,
        |i| &mut i.agent_uid,
        |i| &mut i.build_uid,
    ];
    for field in fields {
        for value in [0, u32::MAX] {
            let mut ids = IDS;
            *field(&mut ids) = value;
            assert_eq!(
                document(&r, &ids, Some([1; 16])),
                Err(InstallError::Account)
            );
        }
    }
    for (a, b) in [(0, 2), (0, 4), (0, 5), (2, 4), (2, 5), (4, 5)] {
        let mut ids = IDS;
        let value = *fields[a](&mut ids);
        *fields[b](&mut ids) = value;
        assert_eq!(
            document(&r, &ids, Some([1; 16])),
            Err(InstallError::Account)
        );
    }
    // The runtime loader accepts a shared control and daemon group.
    let mut ids = IDS;
    ids.owner_gid = ids.daemon_gid;
    assert!(document(&r, &ids, Some([1; 16])).is_ok());
}

#[test]
fn account_names_are_bounded_and_nul_free() {
    type Field = fn(&mut Request) -> &mut String;
    let fields: [Field; 4] = [
        |r| &mut r.operator_account,
        |r| &mut r.control_group,
        |r| &mut r.agent_account,
        |r| &mut r.build_account,
    ];
    for field in fields {
        for bad in [String::new(), "a\0b".into(), "a".repeat(257)] {
            let mut r = request(Role::Founder);
            *field(&mut r) = bad;
            assert_eq!(check(&r), Err(InstallError::Argument));
        }
        let mut r = request(Role::Founder);
        *field(&mut r) = "a".repeat(256);
        assert_eq!(check(&r), Ok(()));
    }
}

#[test]
fn oversized_documents_are_refused() {
    let mut r = request(Role::Founder);
    let long = format!("/{}", vec!["d".repeat(255); 15].join("/"));
    for (i, field) in [
        &mut r.workspace,
        &mut r.state,
        &mut r.credentials,
        &mut r.wrap,
        &mut r.history,
    ]
    .into_iter()
    .enumerate()
    {
        *field = format!("{long}/{i}");
    }
    r.custody = format!("{long}/c/custody");
    r.out = format!("{long}/out/cluster.json");
    assert_eq!(check(&r), Ok(()));
    assert_eq!(
        document(&r, &IDS, Some([1; 16])),
        Err(InstallError::Argument)
    );
}

#[test]
fn intents_and_tokens_are_fixed() {
    assert_eq!(
        INTENTS,
        [
            "create-founder",
            "join",
            "recover-founder",
            "recover-joiner"
        ]
    );
    assert_eq!(DAEMON_ACCOUNT, "arxd-cluster");
    for (error, token, code) in [
        (InstallError::Argument, "cluster-config-argument", 3),
        (InstallError::Account, "cluster-config-account", 3),
        (InstallError::Address, "cluster-config-address", 3),
        (InstallError::Layout, "cluster-config-layout", 3),
        (
            InstallError::OwnerRequired,
            "cluster-config-owner-required",
            4,
        ),
        (InstallError::Exists, "cluster-config-exists", 1),
        (InstallError::Io, "cluster-config-io", 1),
        (InstallError::Unsupported, "cluster-config-unsupported", 7),
    ] {
        assert_eq!(error.token(), token);
        assert_eq!(error.to_string(), token);
        assert_eq!(error.exit_code(), code);
    }
}
