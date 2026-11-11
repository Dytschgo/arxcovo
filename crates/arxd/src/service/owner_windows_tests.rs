use super::*;
static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Fixture {
    dir: tempfile::TempDir,
    _serial: std::sync::MutexGuard<'static, ()>,
}
impl Fixture {
    fn path(&self) -> &Path {
        self.dir.path()
    }
}
fn fixture() -> (Fixture, ProtectionPolicy) {
    let serial = FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let who = crate::local::current_identity().unwrap();
    let policy = ProtectionPolicy {
        fixture_owner: Some(who.clone()),
    };
    let sd = descriptor(&format!("O:{who}D:P(A;OICI;FA;;;{who})")).unwrap();
    unsafe {
        let (mut owner, mut acl) = (null_mut(), null_mut());
        let (mut present, mut defaulted) = (0, 0);
        assert_ne!(
            GetSecurityDescriptorOwner(sd.0, &mut owner, &mut defaulted),
            0
        );
        assert_ne!(
            GetSecurityDescriptorDacl(sd.0, &mut present, &mut acl, &mut defaulted),
            0
        );
        let mut path: Vec<u16> = dir
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        assert_eq!(
            SetNamedSecurityInfoW(
                path.as_mut_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
                owner,
                null_mut(),
                acl,
                null_mut()
            ),
            0
        );
    }
    let sd = descriptor(&policy.creation_descriptor()).unwrap();
    drop(
        raw_open(
            &dir.path().join("owner.lock"),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            CREATE_NEW,
            Some(&sd),
        )
        .unwrap(),
    );
    (
        Fixture {
            dir,
            _serial: serial,
        },
        policy,
    )
}
fn session(root: &Path, policy: ProtectionPolicy) -> Result<WindowsOwnerStore, Failure> {
    // Fixture authority is only this test's private parent. Validate the
    // shared path without retaining global temporary-directory handles; the
    // production constructor still retains its complete protected chain.
    for ancestor in root
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let pin = raw_open(
            ancestor,
            READ_CONTROL | FILE_READ_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            OPEN_EXISTING,
            None,
        )?;
        structural(&pin, true)?;
        drop(pin);
    }
    let pin = raw_open(
        root,
        READ_CONTROL | FILE_READ_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_EXISTING,
        None,
    )?;
    policy.check(&pin, true)?;
    WindowsOwnerStore::locked(
        root.to_owned(),
        Layout::for_test(Platform::Windows),
        policy,
        vec![pin],
    )
}
fn open_fixture() -> (Fixture, WindowsOwnerStore) {
    let (d, p) = fixture();
    let s = session(d.path(), p).unwrap();
    (d, s)
}
fn record() -> OwnerRecord {
    OwnerRecord::decode(br#"{"version":1,"sid":"S-1-5-21-1-2-3-1001","state":"none"}"#).unwrap()
}
#[test]
fn n1_create_new_and_existing_refusal() {
    let (d, s) = open_fixture();
    let staged = s
        .stage("owner-record.json", b"original", None, 512)
        .unwrap();
    let id = identity(&staged.file).unwrap();
    s.publish(staged).unwrap();
    let f = raw_open(
        &d.path().join("owner-record.json"),
        GENERIC_READ,
        FILE_SHARE_READ,
        OPEN_EXISTING,
        None,
    )
    .unwrap();
    assert_eq!(identity(&f).unwrap(), id);
    drop(f);
    assert_eq!(
        std::fs::read(d.path().join("owner-record.json")).unwrap(),
        b"original"
    );
    assert!(matches!(
        s.stage("owner-record.json", b"other", None, 512),
        Err(Failure::Conflict)
    ));
    assert!(!d.path().join("owner-record.json.staging").exists());
}
#[test]
fn n2_exact_hash_replace_and_conflict_cleanup() {
    let (d, s) = open_fixture();
    s.publish(
        s.stage("arxd.json", b"first", None, MAX_CONFIG_BYTES)
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        s.stage(
            "arxd.json",
            b"second",
            Some(digest(b"wrong")),
            MAX_CONFIG_BYTES
        ),
        Err(Failure::Conflict)
    ));
    assert_eq!(std::fs::read(d.path().join("arxd.json")).unwrap(), b"first");
    assert!(!d.path().join("arxd.json.staging").exists());
    s.publish(
        s.stage(
            "arxd.json",
            b"second",
            Some(digest(b"first")),
            MAX_CONFIG_BYTES,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(d.path().join("arxd.json")).unwrap(),
        b"second"
    );
}
#[test]
fn n3_hardlinked_target_and_staging_refuse() {
    let (d, s) = open_fixture();
    let target = d.path().join("arxd.json");
    s.publish(s.stage("arxd.json", b"first", None, 100).unwrap())
        .unwrap();
    std::fs::hard_link(&target, d.path().join("alias")).unwrap();
    assert!(
        s.stage("arxd.json", b"other", Some(digest(b"first")), 100)
            .is_err()
    );
    std::fs::remove_file(d.path().join("alias")).unwrap();
    let staged = s
        .stage("arxd.json", b"other", Some(digest(b"first")), 100)
        .unwrap();
    std::fs::hard_link(d.path().join("arxd.json.staging"), d.path().join("alias")).unwrap();
    assert!(s.publish(staged).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"first");
}
#[test]
fn n3_junction_parent_refuses() {
    use std::os::windows::process::CommandExt;
    let (d, _) = fixture();
    std::fs::create_dir(d.path().join("target")).unwrap();
    let cmd = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe");
    let output = std::process::Command::new(cmd)
        .current_dir(d.path())
        .raw_arg(format!(
            "/d /c mklink /J link \"{}\"",
            d.path().join("target").display()
        ))
        .output()
        .unwrap();
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "junction fixture: exit={:?}, access_denied={}, syntax={}, privilege={}",
        output.status.code(),
        message.contains("denied"),
        message.contains("syntax"),
        message.contains("privilege")
    );
    let p = ProtectionPolicy {
        fixture_owner: Some(crate::local::current_identity().unwrap()),
    };
    assert!(session(&d.path().join("link/child"), p).is_err());
    let f = raw_open(
        &d.path().join("link"),
        READ_CONTROL | FILE_READ_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_EXISTING,
        None,
    )
    .unwrap();
    assert!(structural(&f, true).is_err());
    drop(f);
    std::fs::remove_dir(d.path().join("link")).unwrap();
}
#[test]
fn n4_parent_pin_allows_publication_blocks_parent_rename() {
    let (d, s) = open_fixture();
    let moved = d.path().with_extension("moved");
    assert!(std::fs::rename(d.path(), &moved).is_err());
    assert!(
        raw_open(
            d.path(),
            DELETE | FILE_READ_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            OPEN_EXISTING,
            None
        )
        .is_err()
    );
    s.publish(s.stage("arxd.json", b"first", None, 100).unwrap())
        .unwrap();
    drop(s);
    std::fs::rename(d.path(), &moved).unwrap();
    std::fs::rename(&moved, d.path()).unwrap();
}
#[test]
fn n5_lock_excludes_second_session_until_drop() {
    let (d, s) = open_fixture();
    let p = || ProtectionPolicy {
        fixture_owner: Some(crate::local::current_identity().unwrap()),
    };
    assert!(matches!(session(d.path(), p()), Err(Failure::Busy)));
    drop(s);
    assert!(session(d.path(), p()).is_ok());
}
#[test]
fn n6_cleanup_checks_policy_keeps_foreign_ace() {
    let (d, s) = open_fixture();
    let staged = s.stage("arxd.json", b"first", None, 100).unwrap();
    drop(staged);
    drop(s);
    let p = || ProtectionPolicy {
        fixture_owner: Some(crate::local::current_identity().unwrap()),
    };
    let s = session(d.path(), p()).unwrap();
    assert!(!d.path().join("arxd.json.staging").exists());
    drop(s);
    let who = crate::local::current_identity().unwrap();
    let sd = descriptor(&format!("O:{who}D:P(A;;FA;;;{who})(A;;FR;;;WD)")).unwrap();
    drop(
        raw_open(
            &d.path().join("arxd.json.staging"),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            CREATE_NEW,
            Some(&sd),
        )
        .unwrap(),
    );
    assert!(matches!(
        session(d.path(), p()),
        Err(Failure::RecoveryRequired)
    ));
    assert!(d.path().join("arxd.json.staging").exists());
}
#[test]
fn n7_caps_and_same_handle_record_hash() {
    let (d, mut s) = open_fixture();
    assert!(
        s.stage("owner-record.json", &[b' '; 513], None, 512)
            .is_err()
    );
    assert!(
        s.stage(
            "arxd.json",
            &vec![0; MAX_CONFIG_BYTES + 1],
            None,
            MAX_CONFIG_BYTES
        )
        .is_err()
    );
    assert!(s.read_config().unwrap().is_none());
    s.publish(
        s.stage("arxd.json", b"exact bytes", None, MAX_CONFIG_BYTES)
            .unwrap(),
    )
    .unwrap();
    assert!(s.read_config().unwrap().unwrap() == b"exact bytes");
    std::fs::write(d.path().join("arxd.json"), vec![0; MAX_CONFIG_BYTES + 1]).unwrap();
    assert!(matches!(s.read_config(), Err(Failure::RecoveryRequired)));
    let h = s.replace_record(None, &record()).unwrap();
    assert_eq!(s.record().unwrap().unwrap().1, h);
    std::fs::write(d.path().join("owner-record.json"), [b' '; 513]).unwrap();
    assert!(matches!(s.record(), Err(Failure::RecoveryRequired)));
}
#[test]
fn n8_foreign_writer_refuses_without_change() {
    let (d, s) = open_fixture();
    s.publish(s.stage("arxd.json", b"first", None, 100).unwrap())
        .unwrap();
    let writer = raw_open(
        &d.path().join("arxd.json"),
        GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        OPEN_EXISTING,
        None,
    )
    .unwrap();
    assert!(
        s.stage("arxd.json", b"other", Some(digest(b"first")), 100)
            .is_err()
    );
    assert_eq!(std::fs::read(d.path().join("arxd.json")).unwrap(), b"first");
    drop(writer);
    s.publish(
        s.stage("arxd.json", b"other", Some(digest(b"first")), 100)
            .unwrap(),
    )
    .unwrap();
}
#[test]
fn n9_exact_system_descriptor_and_default_owner() {
    use layout::{Ace, Protection};
    let sd = descriptor(PRIVATE_DESCRIPTOR).unwrap();
    let p = native::windows_descriptor_protection(sd.0).unwrap();
    match p {
        Protection::Windows {
            owner,
            protected,
            dacl_present,
            aces,
        } => {
            assert!(owner == "S-1-5-18" && protected && dacl_present);
            assert_eq!(aces.len(), 2);
            assert!(
                matches!(&aces[0],Ace::Allow{sid,mask,inherit_only:false} if sid=="S-1-5-18"&&*mask==FILE_ALL_ACCESS)
            );
            assert!(
                matches!(&aces[1],Ace::Allow{sid,mask,inherit_only:false} if sid=="S-1-5-32-544"&&*mask==FILE_ALL_ACCESS)
            );
        }
        _ => panic!("descriptor platform"),
    }
    for text in [
        "O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)",
        "O:SYG:SYD:(A;;FA;;;SY)(A;;FA;;;BA)",
        "O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;WD)",
    ] {
        let sd = descriptor(text).unwrap();
        let p = native::windows_descriptor_protection(sd.0).unwrap();
        assert!(
            layout::check_protection(Platform::Windows, Policy::PrivateFile, true, &p).is_err()
        );
    }
    // Explicit O:SY does not depend on the token's default owner. No disk object.
    let mut owner = null_mut();
    let mut defaulted = 0;
    assert_ne!(
        unsafe { GetSecurityDescriptorOwner(sd.0, &mut owner, &mut defaulted) },
        0
    );
    assert_eq!(defaulted, 0);
}
#[test]
fn n10_synthetic_lsa_only() {
    assert_eq!(mutation_status(0), Ok(()));
    for status in [-1, STATUS_ACCESS_DENIED, STATUS_OBJECT_NAME_NOT_FOUND] {
        assert_eq!(mutation_status(status), Err(Failure::Io));
    }
    unsafe {
        assert_eq!(
            observed(STATUS_OBJECT_NAME_NOT_FOUND, null(), 99),
            Ok(false)
        );
        assert_eq!(observed(0, null(), 0), Ok(false));
        assert_eq!(observed(-1, null(), 0), Err(Failure::Io));
        assert_eq!(observed(0, null(), 1), Err(Failure::Io));
        let mut name = wide("SeBatchLogonRight");
        let mut s = LSA_UNICODE_STRING {
            Length: 34,
            MaximumLength: 36,
            Buffer: name.as_mut_ptr(),
        };
        assert_eq!(observed(0, &s, 1), Ok(true));
        let malformed = LSA_UNICODE_STRING {
            Length: 1,
            MaximumLength: 2,
            Buffer: name.as_mut_ptr(),
        };
        assert_eq!(observed(0, [s, malformed].as_ptr(), 2), Err(Failure::Io));
        assert_eq!(observed(0, &s, 65), Err(Failure::Io));
        s.Length = 31;
        assert_eq!(observed(0, &s, 1), Err(Failure::Io));
        s.Length = 130;
        assert_eq!(observed(0, &s, 1), Err(Failure::Io));
        s.Length = 34;
        s.MaximumLength = 30;
        assert_eq!(observed(0, &s, 1), Err(Failure::Io));
        s.MaximumLength = 36;
        name[0] = b's' as u16;
        assert_eq!(observed(0, &s, 1), Ok(false));
        s.Buffer = null_mut();
        assert_eq!(observed(0, &s, 1), Err(Failure::Io));
    }
}

#[test]
fn create_new_race_and_staged_source_substitution_refuse() {
    let (d, s) = open_fixture();
    let staged = s.stage("arxd.json", b"ours", None, 100).unwrap();
    let sd = descriptor(&s.policy.creation_descriptor()).unwrap();
    let mut other = raw_open(
        &d.path().join("arxd.json"),
        GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_DELETE,
        CREATE_NEW,
        Some(&sd),
    )
    .unwrap();
    other.write_all(b"foreign").unwrap();
    drop(other);
    assert_eq!(s.publish(staged), Err(Failure::Conflict));
    assert_eq!(
        std::fs::read(d.path().join("arxd.json")).unwrap(),
        b"foreign"
    );
    assert!(!d.path().join("arxd.json.staging").exists());
    let staged = s
        .stage("arxd.json", b"ours", Some(digest(b"foreign")), 100)
        .unwrap();
    std::fs::rename(
        d.path().join("arxd.json.staging"),
        d.path().join("displaced"),
    )
    .unwrap();
    let mut impostor = raw_open(
        &d.path().join("arxd.json.staging"),
        GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_DELETE,
        CREATE_NEW,
        Some(&sd),
    )
    .unwrap();
    impostor.write_all(b"ours").unwrap();
    drop(impostor);
    assert_eq!(s.publish(staged), Err(Failure::Conflict));
    assert_eq!(
        std::fs::read(d.path().join("arxd.json")).unwrap(),
        b"foreign"
    );
    assert_eq!(
        std::fs::read(d.path().join("arxd.json.staging")).unwrap(),
        b"ours"
    );
}

#[test]
fn sibling_fixture_sessions_release_only_their_own_parent() {
    let (first, policy) = fixture();
    let Fixture {
        dir: first,
        _serial,
    } = first;
    drop(_serial);
    let (second, other_policy) = fixture();
    let store = session(first.path(), policy).unwrap();
    let other = session(second.path(), other_policy).unwrap();
    drop(store);
    let moved = first.path().with_extension("moved");
    std::fs::rename(first.path(), &moved).expect("dropped fixture parent is released");
    std::fs::rename(&moved, first.path()).unwrap();
    assert!(std::fs::rename(second.path(), second.path().with_extension("moved")).is_err());
    drop(other);
}
