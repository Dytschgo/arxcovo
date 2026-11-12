use super::*;
static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
// A directory rename opens its target parent for write. Concurrent native
// fixtures pin the shared temporary directory without write sharing (ancestor
// rule), so renamed fixture parents live under a test-owned outer directory.
struct Fixture {
    dir: PathBuf,
    _outer: tempfile::TempDir,
    _serial: std::sync::MutexGuard<'static, ()>,
}
impl Fixture {
    fn path(&self) -> &Path {
        &self.dir
    }
}
fn fixture() -> (Fixture, ProtectionPolicy) {
    let serial = FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let outer = tempfile::tempdir().unwrap();
    let dir = outer.path().join("owner");
    let policy = prepare(&dir);
    (
        Fixture {
            dir,
            _outer: outer,
            _serial: serial,
        },
        policy,
    )
}
fn prepare(dir: &Path) -> ProtectionPolicy {
    std::fs::create_dir(dir).unwrap();
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
        let mut path: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
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
            &dir.join("owner.lock"),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            CREATE_NEW,
            Some(&sd),
        )
        .unwrap(),
    );
    policy
}
fn session(root: &Path, policy: ProtectionPolicy) -> Result<WindowsOwnerStore, Failure> {
    let mut pins = pin_private_dirs([root], &policy, |_| Ok(())).map_err(|e| match e {
        OpenError::Store(f) => f,
        _ => Failure::RecoveryRequired,
    })?;
    WindowsOwnerStore::locked(
        root.to_owned(),
        Layout::for_test(Platform::Windows),
        policy,
        pins.split_off(pins.len() - 1),
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
fn same_bytes_target_swap_conflicts_before_publication() {
    for name in ["owner-record.json", "arxd.json"] {
        let (d, s) = open_fixture();
        s.publish(s.stage(name, b"first", None, 100).unwrap())
            .unwrap();
        let staged = s
            .stage(name, b"second", Some(digest(b"first")), 100)
            .unwrap();
        let old_id = identity(staged.old.as_ref().unwrap()).unwrap();
        let target = d.path().join(name);
        let saved = d.path().join("previous");
        std::fs::rename(&target, &saved).unwrap();
        let sd = descriptor(&s.policy.creation_descriptor()).unwrap();
        let mut replacement = raw_open(
            &target,
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            CREATE_NEW,
            Some(&sd),
        )
        .unwrap();
        replacement.write_all(b"first").unwrap();
        replacement.sync_all().unwrap();
        s.policy.check(&replacement, false).unwrap();
        let replacement_id = identity(&replacement).unwrap();
        assert_ne!(old_id, replacement_id);
        drop(replacement);

        assert!(matches!(s.publish(staged), Err(Failure::Conflict)));
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        assert_eq!(std::fs::read(&saved).unwrap(), b"first");
        assert!(!d.path().join(format!("{name}.staging")).exists());
        let current = s.target(&target, Some(digest(b"first")), 100).unwrap();
        assert_eq!(identity(current.as_ref().unwrap()).unwrap(), replacement_id);
        drop(current);
        // A new transaction against the replacement is valid; only the stale
        // transaction must refuse, even though its content hash still matches.
        s.publish(
            s.stage(name, b"second", Some(digest(b"first")), 100)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"second");
    }
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
    // Both parents share one owned directory, so a session that retained an
    // ancestor pin would still block its sibling's rename.
    let _serial = FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let outer = tempfile::tempdir().unwrap();
    let (first, second) = (outer.path().join("first"), outer.path().join("second"));
    let (policy, other_policy) = (prepare(&first), prepare(&second));
    let store = session(&first, policy).unwrap();
    let other = session(&second, other_policy).unwrap();
    drop(store);
    let moved = first.with_extension("moved");
    std::fs::rename(&first, &moved).expect("dropped fixture parent is released");
    std::fs::rename(&moved, &first).unwrap();
    assert!(std::fs::rename(&second, second.with_extension("moved")).is_err());
    drop(other);
}

struct CheckedRight<F> {
    callback: F,
    calls: Vec<&'static str>,
}
impl<F: FnMut()> BatchRight for CheckedRight<F> {
    fn observe(&mut self, _: &str) -> Result<bool, Failure> {
        (self.callback)();
        self.calls.push("observe");
        Ok(true)
    }
    fn grant(&mut self, _: &str) -> Result<(), Failure> {
        panic!("grant forbidden")
    }
    fn revoke(&mut self, _: &str) -> Result<(), Failure> {
        (self.callback)();
        self.calls.push("revoke");
        Ok(())
    }
}
fn granted() -> OwnerRecord {
    OwnerRecord::decode(br#"{"version":1,"sid":"S-1-5-21-1-2-3-1001","state":"granted"}"#).unwrap()
}
fn private_fixture(
    root: &Path,
    policy: ProtectionPolicy,
    ancestor: impl Fn(&layout::Protection) -> Result<(), Error>,
) -> Result<WindowsOwnerStore, OpenError> {
    let mut pins = pin_private_dirs([root], &policy, ancestor)?;
    // Only root is fixture authority. The shared walk checks the entire chain;
    // retaining shared-temp ancestor handles would fence unrelated fixtures.
    let private_pins = pins.split_off(pins.len() - 1);
    drop(pins);
    Ok(WindowsOwnerStore::locked(
        root.to_owned(),
        Layout::for_test(Platform::Windows),
        policy,
        private_pins,
    )?)
}
#[test]
fn u2_u3_private_pins_cover_configure_and_uninstall() {
    for uninstall in [false, true] {
        let (d, policy) = fixture();
        let mut s = private_fixture(d.path(), policy, |_| Ok(())).unwrap();
        s.replace_record(None, &granted()).unwrap();
        let moved = d.path().with_file_name("moved");
        let mut right = CheckedRight {
            callback: || {
                assert!(std::fs::rename(d.path(), &moved).is_err());
                assert!(matches!(
                    raw_open(
                        &d.path().join("owner.lock"),
                        READ_CONTROL | FILE_READ_DATA,
                        0,
                        OPEN_EXISTING,
                        None
                    ),
                    Err(Failure::Busy)
                ));
            },
            calls: vec![],
        };
        if uninstall {
            assert!(matches!(
                uninstall_locked(
                    super::super::owner_scm::fixture_deregistered(),
                    &mut s,
                    &mut right
                ),
                Ok(RightReport::Removed)
            ));
            assert_eq!(right.calls, ["observe", "revoke"]);
        } else {
            owner_transactions::recover(&mut s, &mut right).unwrap();
            owner_transactions::observe_batch("S-1-5-21-1-2-3-1001", &mut s, &mut right).unwrap();
            assert_eq!(right.calls, ["observe"]);
        }
        drop(s);
        std::fs::rename(d.path(), &moved).unwrap();
        std::fs::rename(&moved, d.path()).unwrap();
        assert!(
            raw_open(
                &d.path().join("owner.lock"),
                READ_CONTROL | FILE_READ_DATA,
                0,
                OPEN_EXISTING,
                None
            )
            .is_ok()
        );
    }
}
#[test]
fn u2_recovery_precedes_uninstall_and_failure_stops_right() {
    let (d, mut s) = open_fixture();
    let intent =
        OwnerRecord::decode(br#"{"version":1,"sid":"S-1-5-21-1-2-3-1001","state":"intent"}"#)
            .unwrap();
    s.replace_record(None, &intent).unwrap();
    let mut right = CheckedRight {
        callback: || (),
        calls: vec![],
    };
    assert!(matches!(
        uninstall_locked(
            super::super::owner_scm::fixture_deregistered(),
            &mut s,
            &mut right
        ),
        Ok(RightReport::RetainedUnproven)
    ));
    assert_eq!(right.calls, ["observe", "observe"]);
    std::fs::write(d.path().join("owner-record.json"), b"invalid").unwrap();
    right.calls.clear();
    assert!(matches!(
        uninstall_locked(
            super::super::owner_scm::fixture_deregistered(),
            &mut s,
            &mut right
        ),
        Err(Failure::RecoveryRequired)
    ));
    assert!(right.calls.is_empty());
}
#[test]
fn u3_unsafe_private_chains_stop_before_store_or_right() {
    for uninstall in [false, true] {
        for case in ["missing", "unsafe", "untrusted", "replaced", "reparse"] {
            let (d, mut policy) = fixture();
            // A staging sentinel must survive refusal before store cleanup.
            std::fs::write(d.path().join("arxd.json.staging"), b"sentinel").unwrap();
            let missing = d.path().join("missing");
            let replacement = d.path().join("replacement");
            let path = match case {
                "missing" => missing.as_path(),
                "unsafe" => {
                    policy.fixture_owner = Some("S-1-0-0".into());
                    d.path()
                }
                "replaced" => {
                    let _ = prepare(&replacement);
                    let previous = raw_open(
                        &replacement,
                        READ_CONTROL | FILE_READ_DATA,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        OPEN_EXISTING,
                        None,
                    )
                    .unwrap();
                    let previous_id = identity(&previous).unwrap();
                    drop(previous);
                    std::fs::rename(&replacement, d.path().join("saved-directory")).unwrap();
                    std::fs::write(&replacement, b"file instead of directory").unwrap();
                    let replaced = raw_open(
                        &replacement,
                        READ_CONTROL | FILE_READ_DATA,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        OPEN_EXISTING,
                        None,
                    )
                    .unwrap();
                    assert_ne!(previous_id, identity(&replaced).unwrap());
                    drop(replaced);
                    replacement.as_path()
                }
                "reparse" => {
                    // A junction needs no symlink privilege and only targets owned temp.
                    let status = std::process::Command::new("cmd")
                        .args(["/c", "mklink", "/J"])
                        .arg(&replacement)
                        .arg(d.path())
                        .output()
                        .unwrap();
                    assert!(status.status.success());
                    replacement.as_path()
                }
                _ => d.path(),
            };
            let mut calls = Vec::new();
            let result = (|| -> Result<(), OpenError> {
                let mut s = private_fixture(path, policy, |_| {
                    if case == "untrusted" {
                        Err(Error::Permissions)
                    } else {
                        Ok(())
                    }
                })?;
                calls.push("store");
                let mut right = CheckedRight {
                    callback: || (),
                    calls: vec![],
                };
                if uninstall {
                    uninstall_locked(
                        super::super::owner_scm::fixture_deregistered(),
                        &mut s,
                        &mut right,
                    )?;
                } else {
                    owner_transactions::recover(&mut s, &mut right)?;
                }
                Ok(())
            })();
            if case == "missing" {
                assert!(matches!(result, Err(OpenError::Store(Failure::Io))));
            } else {
                assert!(matches!(result, Err(OpenError::Layout(Error::Permissions))));
            }
            assert!(calls.is_empty());
            assert_eq!(
                std::fs::read(d.path().join("arxd.json.staging")).unwrap(),
                b"sentinel"
            );
            if case == "reparse" {
                std::fs::remove_dir(&replacement).unwrap();
            }
        }
    }
}

// Small lexer for this Rust source: comments and all quoted literals are
// discarded before balanced item/call inspection. No source substring matches.
fn code_tokens(source: &str) -> Vec<String> {
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
                assert!(i < b.len());
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
        // Raw strings, including byte raw strings.
        let raw = if b[i..].starts_with(b"br") {
            i + 2
        } else if b[i] == b'r' {
            i + 1
        } else {
            i
        };
        let mut quote = raw;
        while quote < b.len() && b[quote] == b'#' {
            quote += 1;
        }
        if raw != i && quote < b.len() && b[quote] == b'"' {
            let hashes = quote - raw;
            i = quote + 1;
            loop {
                assert!(i < b.len());
                if b[i] == b'"'
                    && b.get(i + 1..i + 1 + hashes)
                        .is_some_and(|h| h.iter().all(|c| *c == b'#'))
                {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        // Character literals (leave lifetime identifiers as code).
        if b[i] == 39
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
            continue;
        }
        let start = i;
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
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
fn balanced(tokens: &[String], start: usize, left: &str, right: &str) -> usize {
    assert_eq!(tokens[start], left);
    let mut depth = 0;
    for (i, token) in tokens.iter().enumerate().skip(start) {
        if token == left {
            depth += 1;
        }
        if token == right {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    panic!("unbalanced source")
}
fn function_body<'a>(tokens: &'a [String], name: &str) -> &'a [String] {
    let indices: Vec<_> = tokens
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] == "fn" && w[1] == name)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(indices.len(), 1, "unique function {name}");
    let start = indices[0] + tokens[indices[0]..].iter().position(|t| t == "{").unwrap();
    &tokens[start + 1..balanced(tokens, start, "{", "}")]
}
fn sequence(tokens: &[String], source: &str) -> usize {
    let expected = code_tokens(source);
    let hits: Vec<_> = tokens
        .windows(expected.len())
        .enumerate()
        .filter(|(_, w)| *w == expected)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(hits.len(), 1, "unique code sequence: {source}");
    hits[0]
}
#[test]
fn s1_production_private_pin_calls_are_bound_before_privilege_and_lock() {
    let tokens = code_tokens(include_str!("owner_windows.rs"));
    let calls = tokens
        .windows(2)
        .enumerate()
        .filter(|(i, w)| {
            w[0] == "pin_private_dirs" && w[1] == "(" && (*i == 0 || tokens[*i - 1] != "fn")
        })
        .count();
    assert_eq!(calls, 2);
    let implementation = sequence(&tokens, "impl WindowsOwnerSession {");
    assert!(matches!(tokens[implementation - 1].as_str(), "}" | ";"));
    let end = balanced(&tokens, implementation + 2, "{", "}");
    for name in ["open", "uninstall"] {
        let at = sequence(&tokens, &format!("pub fn {name} ("));
        assert!(implementation < at && at < end);
        assert!(matches!(tokens[at - 1].as_str(), "{" | "}"));
        let prefix = &tokens[implementation + 3..at];
        assert_eq!(
            prefix.iter().filter(|t| *t == "{").count(),
            prefix.iter().filter(|t| *t == "}").count()
        );
        let body = function_body(&tokens, name);
        let policy = sequence(body, "let policy = ProtectionPolicy::production();");
        let pin = sequence(
            body,
            "pins.extend(pin_private_dirs(layout.private_dirs(), &policy, layout::check_windows_ancestor,)?);",
        );
        let restore = sequence(body, "enable_restore()?");
        let lock = sequence(body, "let lock = optional(");
        assert!(policy < pin && pin < restore && restore < lock);
        let store = sequence(
            body,
            "WindowsOwnerStore::with_lock(root, layout, policy, pins, lock)?",
        );
        assert!(lock < store, "private pins must reach the locked store");
        // A conditional/test-only/nested call cannot satisfy the production binding.
        assert_eq!(
            body[..pin].iter().filter(|t| *t == "{").count(),
            body[..pin].iter().filter(|t| *t == "}").count()
        );
        assert!(!body[..restore].iter().any(|t| t == "cfg" || t == "return"));
    }
    assert!(!tokens.windows(2).any(|w| w == ["fn", "parts"]));
    assert_eq!(
        tokens
            .windows(2)
            .filter(|w| w[0] == "uninstall_right" && w[1] == "(")
            .count(),
        1
    );
    sequence(
        function_body(&tokens, "uninstall_locked"),
        "owner_transactions::recover(store, right)?; owner_transactions::uninstall_right(store, right)",
    );
    // Struct fields drop in declaration order. Staging and pins precede lock.
    let start = sequence(&tokens, "struct WindowsOwnerStore {") + 3;
    let fields = &tokens[start..balanced(&tokens, start - 1, "{", "}")];
    assert!(sequence(fields, "staged:") < sequence(fields, "_pins:"));
    assert!(sequence(fields, "_pins:") < sequence(fields, "_lock:"));
    assert_eq!(fields.last().unwrap(), ",");
    assert_eq!(&fields[fields.len() - 4..], code_tokens("_lock: File,"));
    for name in ["WindowsOwnerStore", "WindowsBatchRight"] {
        let at = sequence(&tokens, &format!("struct {name} {{"));
        assert_ne!(tokens[at.saturating_sub(1)], "pub");
    }
}
#[test]
fn s1_lexer_ignores_decoy_comments_and_literals() {
    assert_eq!(
        code_tokens("/* nested /* fn x() {} */ */ \"fn x() {}\" r#\"fn x() {}\"# fn actual() {}"),
        code_tokens("fn actual() {}")
    );
    assert_eq!(
        code_tokens("'¹' 'a' fn actual() {}"),
        code_tokens("fn actual() {}")
    );
}

#[test]
fn overlapping_private_chains_allow_publication_and_pin_directories() {
    let (fixture, _) = fixture();
    let config = fixture.path().join("config");
    let policy = prepare(&config);
    let mut ids = Vec::new();
    for path in [fixture.path(), config.as_path()] {
        let file = raw_open(
            path,
            READ_CONTROL | FILE_READ_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            None,
        )
        .unwrap();
        ids.push(identity(&file).unwrap());
    }
    let mut pins =
        pin_private_dirs([fixture.path(), config.as_path()], &policy, |_| Ok(())).unwrap();
    // Keep every pin inside the owned scope, including the root in both roles.
    // Shared temp ancestors are not fixture authority and must not fence peers.
    pins.retain(|file| ids.contains(&identity(file).unwrap()));
    assert_eq!(pins.len(), 3);
    let mut store = WindowsOwnerStore::locked(
        config.clone(),
        Layout::for_test(Platform::Windows),
        policy,
        pins,
    )
    .unwrap();
    store.replace_record(None, &record()).unwrap();
    assert_eq!(
        store.record().unwrap().unwrap().0.encode().unwrap(),
        record().encode().unwrap()
    );
    let staged = store
        .stage("arxd.json", b"fixed-config-bytes", None, 100)
        .unwrap();
    store.publish(staged).unwrap();
    assert!(std::fs::read(config.join("arxd.json")).unwrap() == b"fixed-config-bytes");
    let moved = fixture.path().join("moved-config");
    assert!(std::fs::rename(&config, &moved).is_err());
    drop(store);
    assert!(std::fs::rename(&config, &moved).is_ok());
    assert!(std::fs::rename(&moved, &config).is_ok());
}

#[test]
fn shared_walk_preserves_binary_and_private_structural_error_layers() {
    let (d, policy) = fixture();
    let file = d.path().join("not-a-directory");
    std::fs::write(&file, b"fixture").unwrap();
    let raw = walk_chain(&file, Some(Policy::PrivateDirectory), &|_| Ok(()), &|_| {
        Ok(())
    });
    assert!(matches!(
        raw,
        Err(OpenError::Store(Failure::RecoveryRequired))
    ));
    let private = pin_private_dirs([&file], &policy, |_| Ok(()));
    assert!(matches!(
        private,
        Err(OpenError::Layout(Error::Permissions))
    ));
}
#[test]
fn s1_installer_source_fences() {
    let source = include_str!("owner_install_windows.rs");
    let tokens = code_tokens(source);
    for forbidden in [
        "BatchRight",
        "owner_transactions",
        "grant",
        "revoke",
        "SetSecurityInfo",
        "SetFileSecurityW",
        "SetNamedSecurityInfoW",
        "rusqlite",
    ] {
        assert!(
            !tokens.iter().any(|t| t == forbidden),
            "installer fence: {forbidden}"
        );
    }
    assert!(!tokens.windows(3).any(|w| w == [".", "start", "("]));
    let existing = function_body(&tokens, "existing");
    sequence(existing, "if access & !READ_ONLY != 0");
    sequence(existing, "open(path, access, share, OPEN_EXISTING, None)");
    let install = function_body(&tokens, "install_with");
    assert!(sequence(install, "ns.binary_check(policy)?") < sequence(install, "scm.observe()?"));
    assert!(sequence(install, "scm.observe()?") < sequence(install, "enable()?"));
    assert!(
        sequence(install, "sidefiles(&state)?; scm.register()?") > sequence(install, "enable()?")
    );
    let tests = code_tokens(include_str!("owner_install_windows_tests.rs"));
    for forbidden in ["acquire", "system", "NativeScm", "enable_restore"] {
        assert!(
            !tests.iter().any(|t| t == forbidden),
            "fixture authority: {forbidden}"
        );
    }
}
// Creation can normalize an unprotected explicit DACL. This test-only helper
// clears protection on one fixed owned-temp object to construct that refusal.
pub(in crate::service) fn unprotect_install_fixture(temp: &tempfile::TempDir) {
    let path = temp.path().join("Arxcovo");
    let mut name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let who = crate::local::current_identity().unwrap();
    let sd = descriptor(&format!("O:{who}D:(A;;FA;;;{who})")).unwrap();
    let (mut acl, mut present, mut defaulted) = (null_mut(), 0, 0);
    assert_ne!(
        unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut acl, &mut defaulted) },
        0
    );
    assert_eq!(
        unsafe {
            SetNamedSecurityInfoW(
                name.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null_mut(),
            )
        },
        0
    );
}

#[test]
fn s1_owner_authority_cannot_be_forged_by_sibling_modules() {
    let tokens = code_tokens(include_str!("owner_windows.rs"));
    let at = sequence(&tokens, "pub struct OwnerGuard {") + 4;
    let fields = &tokens[at..balanced(&tokens, at - 1, "{", "}")];
    assert_eq!(fields, code_tokens("layout: Layout, pins: Vec<File>,"));
    sequence(&tokens, "pub(super) struct Local(*mut core::ffi::c_void);");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let defining = root.join("service/owner_windows.rs");
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink(), "source fence does not follow links");
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if path != defining && path.extension().is_some_and(|e| e == "rs") {
                let tokens = code_tokens(&std::fs::read_to_string(path).unwrap());
                assert!(
                    !tokens.windows(2).any(|w| w == ["OwnerGuard", "{"]),
                    "OwnerGuard construction or destructuring outside its defining module"
                );
            }
        }
    }
}

#[test]
fn configure_session_fake_can_refuse_without_native_calls() {
    struct Refusal;
    impl ConfigureSession for Refusal {
        fn read_config(&mut self) -> Result<Option<Vec<u8>>, Failure> {
            Err(Failure::Busy)
        }
        fn observe_batch(&mut self, _: &str) -> Result<BatchObservation, Failure> {
            Err(Failure::Busy)
        }
        fn configure(&mut self, _: &Configure) -> Result<(), Failure> {
            Err(Failure::Busy)
        }
    }
    let _configure: fn(&mut Refusal, &Configure) -> Result<(), Failure> =
        <Refusal as ConfigureSession>::configure;
    fn read(s: &mut impl ConfigureSession) -> Result<Option<Vec<u8>>, Failure> {
        s.read_config()
    }
    assert!(matches!(read(&mut Refusal), Err(Failure::Busy)));
    assert!(matches!(
        Refusal.observe_batch("synthetic"),
        Err(Failure::Busy)
    ));
}
