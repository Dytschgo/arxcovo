use super::*;

const SID: &str = "S-1-5-21-1-2-3-1002";
fn request() -> Configure {
    Configure {
        bytes: b"new config".to_vec(),
        expected: Some(b"old config".to_vec()),
        sid: SID.into(),
        add: true,
    }
}
struct Recording {
    calls: Vec<&'static str>,
    journal: Option<Journal>,
    right: bool,
    actual: Option<[u8; 32]>,
    staged: Option<[u8; 32]>,
    fail: Option<&'static str>,
    publish_then_fail: bool,
}
impl Default for Recording {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            journal: None,
            right: false,
            actual: Some(hash(b"old config")),
            staged: None,
            fail: None,
            publish_then_fail: false,
        }
    }
}
impl Recording {
    fn call(&mut self, name: &'static str) -> Result<(), Failure> {
        self.calls.push(name);
        if self.fail == Some(name) {
            Err(Failure::Io)
        } else {
            Ok(())
        }
    }
}
impl ConfigurationIo for Recording {
    fn journal(&mut self) -> Result<Option<Journal>, Failure> {
        self.call("read-journal")?;
        Ok(self.journal.clone())
    }
    fn prepare(&mut self, r: &Configure) -> Result<(), Failure> {
        self.call("prepare")?;
        if self.actual != r.expected.as_deref().map(hash) || self.right == r.add {
            return Err(Failure::Conflict);
        }
        self.staged = Some(hash(&r.bytes));
        Ok(())
    }
    fn write_journal(&mut self, j: &Journal) -> Result<(), Failure> {
        self.call(if j.phase == Phase::Intent {
            "intent"
        } else {
            "receipt"
        })?;
        self.journal = Some(Journal::decode(&j.encode()?)?);
        Ok(())
    }
    fn grant_batch(&mut self, sid: &str) -> Result<(), Failure> {
        assert_eq!(sid, SID);
        self.call("grant")?;
        self.right = true;
        Ok(())
    }
    fn revoke_batch(&mut self, sid: &str) -> Result<(), Failure> {
        assert_eq!(sid, SID);
        self.call("revoke")?;
        self.right = false;
        Ok(())
    }
    fn publish(&mut self) -> Result<(), Failure> {
        if self.publish_then_fail {
            self.actual = self.staged;
        }
        self.call("publish")?;
        self.actual = self.staged;
        Ok(())
    }
    fn config_hash(&mut self) -> Result<Option<[u8; 32]>, Failure> {
        self.call("hash")?;
        Ok(self.actual)
    }
    fn clear_journal(&mut self) -> Result<(), Failure> {
        self.call("clear")?;
        self.journal = None;
        Ok(())
    }
}
#[test]
fn staging_and_durable_intent_precede_grant_receipt_precedes_publication() {
    let mut io = Recording::default();
    configure(&request(), &mut io).unwrap();
    assert_eq!(
        io.calls,
        [
            "read-journal",
            "prepare",
            "intent",
            "grant",
            "receipt",
            "publish",
            "clear"
        ]
    );
    assert!(io.right);
    assert!(io.journal.is_none());
    assert_eq!(io.actual, Some(hash(b"new config")));
}
#[test]
fn early_failures_cannot_grant_and_stale_observations_refuse() {
    for fail in ["read-journal", "prepare", "intent"] {
        let mut io = Recording {
            fail: Some(fail),
            ..Default::default()
        };
        assert!(configure(&request(), &mut io).is_err());
        assert!(!io.right);
        assert!(!io.calls.contains(&"grant"));
    }
    for (actual, right) in [
        (Some(hash(b"concurrent")), false),
        (Some(hash(b"old config")), true),
    ] {
        let mut io = Recording {
            actual,
            right,
            ..Default::default()
        };
        assert_eq!(configure(&request(), &mut io), Err(Failure::Conflict));
        assert_eq!(io.calls, ["read-journal", "prepare"]);
    }
}
#[test]
fn ambiguous_lsa_or_receipt_failure_never_infers_ownership_from_presence() {
    for fail in ["grant", "receipt"] {
        let mut io = Recording {
            fail: Some(fail),
            ..Default::default()
        };
        assert_eq!(
            configure(&request(), &mut io),
            Err(Failure::RecoveryRequired)
        );
        assert!(
            io.journal
                .as_ref()
                .is_some_and(|j| j.phase == Phase::Intent)
        );
        for present in [false, true] {
            io.right = present;
            io.calls.clear();
            io.fail = None;
            assert_eq!(recover(&mut io), Err(Failure::RecoveryRequired));
            assert_eq!(io.calls, ["read-journal"]);
            assert_eq!(io.right, present);
            assert_eq!(
                configure(&request(), &mut io),
                Err(Failure::RecoveryRequired)
            );
        }
    }
}
#[test]
fn publication_failure_compensates_only_a_durable_grant_receipt() {
    let mut io = Recording {
        fail: Some("publish"),
        ..Default::default()
    };
    assert_eq!(configure(&request(), &mut io), Err(Failure::Io));
    assert!(!io.right);
    assert!(io.journal.is_none());
    assert_eq!(&io.calls[6..], ["read-journal", "hash", "revoke", "clear"]);
    assert_eq!(io.actual, Some(hash(b"old config")));
}
#[test]
fn ambiguous_publication_retains_right_when_new_config_is_visible() {
    let mut io = Recording {
        fail: Some("publish"),
        publish_then_fail: true,
        ..Default::default()
    };
    configure(&request(), &mut io).unwrap();
    assert!(io.right);
    assert!(io.journal.is_none());
    assert!(!io.calls.contains(&"revoke"));
}
#[test]
fn recovery_conflict_and_failed_compensation_preserve_receipt() {
    for fail in ["hash", "revoke", "clear"] {
        let mut io = Recording {
            fail: Some("clear"),
            ..Default::default()
        };
        assert_eq!(configure(&request(), &mut io), Err(Failure::Io));
        io.actual = Some(hash(b"old config"));
        io.fail = Some(fail);
        io.calls.clear();
        assert!(recover(&mut io).is_err());
        assert!(io.journal.is_some());
        io.fail = None;
        assert_eq!(recover(&mut io), Ok(Recovery::Compensated));
        assert!(!io.right);
        assert!(io.journal.is_none());
    }
    let mut io = Recording {
        fail: Some("clear"),
        ..Default::default()
    };
    assert!(configure(&request(), &mut io).is_err());
    io.fail = None;
    io.actual = Some(hash(b"foreign"));
    io.calls.clear();
    assert_eq!(recover(&mut io), Err(Failure::Conflict));
    assert!(io.right);
    assert!(io.journal.is_some());
    assert_eq!(io.calls, ["read-journal", "hash"]);
}
#[test]
fn existing_right_never_creates_a_grant_receipt_or_compensates() {
    let mut r = request();
    r.add = false;
    let mut io = Recording {
        right: true,
        fail: Some("publish"),
        ..Default::default()
    };
    assert_eq!(configure(&r, &mut io), Err(Failure::Io));
    assert!(io.right);
    assert!(io.journal.is_none());
    assert_eq!(io.calls, ["read-journal", "prepare", "publish"]);
}
#[test]
fn journal_parser_rejects_unbounded_unknown_and_reserved_identities() {
    assert!(Journal::decode(&[0; 1025]).is_err());
    let mut io = Recording {
        fail: Some("receipt"),
        ..Default::default()
    };
    assert!(configure(&request(), &mut io).is_err());
    let bytes = io.journal.unwrap().encode().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for sid in [
        "S-1-5-18",
        "S-1-5-32-544",
        "S-1-5-21-1-2-3-01",
        "S-1-5-21-1-2-3-4294967296",
    ] {
        value["sid"] = sid.into();
        assert!(Journal::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    value["sid"] = SID.into();
    value["injected"] = true.into();
    assert!(Journal::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    assert_eq!(
        Failure::RecoveryRequired.to_string(),
        "service-owner-recovery-required"
    );
}
#[test]
fn plan_extraction_requires_windows_configure_and_validated_build_pin() {
    use crate::service::{
        config::{Account, AccountKind},
        owner_plan::{Action, OwnerContext, configure_plan, owner_plan},
    };
    struct ResolverFixture;
    impl Resolver for ResolverFixture {
        fn account(&self, n: &str) -> Result<Account, crate::service::layout::Error> {
            Ok(Account {
                identity: if n == "builder" { SID } else { n }.into(),
                gid: None,
                kind: AccountKind::User,
                administrative: false,
            })
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, crate::service::layout::Error> {
            Ok(true)
        }
    }
    let l = Layout::for_test(Platform::Windows);
    let c = OwnerContext {
        admin: true,
        running_binary: l.binary(),
    };
    let mut v = crate::service::config::tests::fixture();
    v["client"]["identity"] = "S-1-5-21-1-2-3-1001".into();
    v["build_account"]["identity"] = SID.into();
    v["build_account"]["gid"] = serde_json::Value::Null;
    v["workspace"] = r"D:\work".into();
    let config =
        ServiceConfig::parse(&serde_json::to_vec(&v).unwrap(), &l, &ResolverFixture).unwrap();
    for present in [false, true] {
        let plan = configure_plan(&l, &c, &config, None, present, &ResolverFixture).unwrap();
        let r = Configure::from_plan(&l, &plan, &ResolverFixture).unwrap();
        assert_eq!(r.adds_right(), !present);
        assert_eq!(r.sid(), SID);
        assert!(r.expected().is_none());
        assert!(!r.bytes().is_empty());
    }
    for action in [
        Action::Install,
        Action::Start,
        Action::Stop,
        Action::Status,
        Action::Uninstall,
    ] {
        assert!(
            Configure::from_plan(&l, &owner_plan(&l, &c, action).unwrap(), &ResolverFixture)
                .is_err()
        );
    }
    let plan = configure_plan(&l, &c, &config, None, false, &ResolverFixture).unwrap();
    assert!(
        Configure::from_plan(&Layout::for_test(Platform::Linux), &plan, &ResolverFixture).is_err()
    );
}
