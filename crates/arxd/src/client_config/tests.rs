use super::*;
use std::{cell::RefCell, fs, path::PathBuf};
#[derive(Default)]
struct Observation {
    wipes: usize,
    clean: bool,
    before: Option<(usize, usize)>,
    stable: bool,
    allocations: usize,
}
thread_local! { static OBSERVER: RefCell<Option<Observation>> = const { RefCell::new(None) }; }
struct AllocationObserver;
impl AllocationObserver {
    fn register() -> Self {
        OBSERVER.with(|o| {
            assert!(o.borrow().is_none(), "observer scope");
            *o.borrow_mut() = Some(Observation {
                clean: true,
                stable: true,
                ..Observation::default()
            });
        });
        Self
    }
    fn check(&self, expected_wipes: usize, allocations: usize) {
        OBSERVER.with(|o| {
            let o = o.borrow();
            let o = o.as_ref().unwrap();
            assert!(o.clean, "wipe incomplete");
            assert!(o.wipes == expected_wipes, "wipe count");
            assert!(o.stable, "JSON allocation changed");
            assert!(o.allocations == allocations, "serialization count");
        });
    }
}
impl Drop for AllocationObserver {
    fn drop(&mut self) {
        OBSERVER.with(|o| *o.borrow_mut() = None);
    }
}
pub(super) fn observe_wipe(clean: bool) {
    OBSERVER.with(|o| {
        if let Some(o) = o.borrow_mut().as_mut() {
            o.wipes += 1;
            o.clean &= clean;
        }
    });
}
pub(super) fn observe_allocation(before: bool, pointer: usize, capacity: usize) {
    OBSERVER.with(|o| {
        if let Some(o) = o.borrow_mut().as_mut() {
            if before {
                o.before = Some((pointer, capacity));
            } else {
                o.stable &= o.before == Some((pointer, capacity));
                o.allocations += 1;
            }
        }
    });
}
const UNIX: &str = "/run/arxcovo/arxd.sock";
const WINDOWS: &str = r"\\.\pipe\arxcovo-arxd";
fn random(raw: &mut [u8; 32]) -> Result<(), ClientError> {
    for (i, b) in raw.iter_mut().enumerate() {
        *b = 0xa5 ^ i as u8;
    }
    Ok(())
}
struct Fixture {
    _temp: tempfile::TempDir,
    parent: PathBuf,
    out: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap_or_else(|_| panic!("owned temporary directory"));
        // Canonicalize only the test-owned base (macOS /var is a symlink).
        let root = temp
            .path()
            .canonicalize()
            .unwrap_or_else(|_| panic!("fixture root"));
        #[cfg(windows)]
        let root = PathBuf::from(
            root.to_str()
                .unwrap()
                .strip_prefix(r"\\?\")
                .unwrap_or(root.to_str().unwrap()),
        );
        let parent = root.join("private");
        let out = parent.join("credential.json").to_str().unwrap().to_owned();
        Self {
            _temp: temp,
            parent,
            out,
        }
    }
    fn run(&self, ops: &mut impl PostCreateOps) -> Result<String, ClientError> {
        create_with(
            "fixture-agent",
            &self.out,
            UNIX,
            "0",
            || Ok(false),
            random,
            ops,
        )
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(&self.out).unwrap_or_else(|_| panic!("read synthetic fixture"))
    }
    fn private(&self) {
        assert!(
            crate::security::private(std::path::Path::new(&self.out)).is_ok(),
            "private file oracle"
        );
        assert!(
            crate::security::private(&self.parent).is_ok(),
            "private parent oracle"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m = fs::metadata(&self.out).unwrap();
            assert!(
                m.nlink() == 1 && m.uid() == unsafe { libc::geteuid() },
                "file ownership/link"
            );
            assert!(m.mode() & 0o077 == 0, "file privacy");
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsHandle;
            let f = fs::File::open(&self.out).unwrap();
            assert!(
                platform::NativeFileOps::new()
                    .unwrap()
                    .verify(f.as_handle(), f.metadata().unwrap().len())
                    .is_ok(),
                "file owner/link/ACL"
            );
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    #[cfg(unix)]
    Mode,
    Write,
    Flush,
    #[cfg(unix)]
    SyncParent,
    Verify,
}
fn steps() -> Vec<Step> {
    vec![
        #[cfg(unix)]
        Step::Mode,
        Step::Write,
        Step::Flush,
        #[cfg(unix)]
        Step::SyncParent,
        Step::Verify,
    ]
}
fn error() -> io::Error {
    io::Error::other("synthetic operation failure")
}
struct Ops {
    native: platform::NativeFileOps,
    calls: Vec<Step>,
    fail: Option<Step>,
    prefix: bool,
    acquisition: bool,
}
impl Ops {
    fn new() -> Self {
        Self {
            native: platform::NativeFileOps::new().unwrap_or_else(|_| panic!("current user facts")),
            calls: Vec::new(),
            fail: None,
            prefix: false,
            acquisition: false,
        }
    }
    fn result(&mut self, step: Step) -> io::Result<()> {
        self.calls.push(step);
        if self.fail == Some(step) {
            Err(error())
        } else {
            Ok(())
        }
    }
}
impl PostCreateOps for Ops {
    #[cfg(unix)]
    fn mode(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        self.result(Step::Mode)?;
        self.native.mode(f)
    }
    fn write(&mut self, f: ClientHandle<'_>, b: &[u8]) -> io::Result<()> {
        self.result(Step::Write)?;
        if self.prefix {
            self.native.write(f, &b[..13])?;
            Err(error())
        } else {
            self.native.write(f, b)
        }
    }
    fn flush(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        self.result(Step::Flush)?;
        self.native.flush(f)
    }
    #[cfg(unix)]
    fn sync_parent(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        self.result(Step::SyncParent)?;
        self.native.sync_parent(f)
    }
    fn verify(&mut self, f: ClientHandle<'_>, n: u64) -> io::Result<()> {
        self.result(Step::Verify)?;
        if self.acquisition {
            self.native.verify_with(|| Err(error()), n)
        } else {
            self.native.verify(f, n)
        }
    }
}
#[test]
fn lexical_matrix_and_order() {
    for p in [r"C:\a\b.json", r"d:\space name\b", r"C:\COM0\x"] {
        assert!(lexical(p, true), "valid windows lexical");
    }
    for p in [
        r"C:\a\..\b",
        r"C:\a\.\b",
        r"C:\a\\b",
        r"C:\a\b:",
        r"C:\NUL.txt\x",
        r"C:\COM¹\x",
        r"C:\a.\x",
        r"C:\a \x",
        r"C:\a\b?",
        r"C:/a/b",
        r"\\server\x",
        r"\\?\C:\a",
        "C:\\a\\",
    ] {
        assert!(!lexical(p, true), "invalid windows lexical");
    }
    for p in ["/a/b", "/file"] {
        assert!(lexical(p, false), "valid Unix lexical");
    }
    for p in [
        "//a/b", "/a/../b", "/a/./b", "/a//b", "/a/", "relative", "/a\\b", "/a\0b",
    ] {
        assert!(!lexical(p, false), "invalid Unix lexical");
    }
    assert!(!lexical(&format!("/{}", "a".repeat(4096)), false));
    let f = Fixture::new();
    let mut ops = Ops::new();
    assert!(matches!(
        create_with(
            "arxd",
            &f.out,
            UNIX,
            "0",
            || panic!("elevation before principal"),
            |_| panic!("RNG before arguments"),
            &mut ops
        ),
        Err(ClientError::Argument)
    ));
    assert!(matches!(
        create_with(
            "agent",
            "relative",
            UNIX,
            "0",
            || panic!("elevation before path"),
            |_| panic!("RNG before path"),
            &mut ops
        ),
        Err(ClientError::Argument)
    ));
    assert!(matches!(
        create_with(
            "agent",
            &f.out,
            UNIX,
            "0",
            || Ok(true),
            |_| panic!("RNG after elevation"),
            &mut ops
        ),
        Err(ClientError::Elevated)
    ));
    assert!(
        !f.parent.exists() && ops.calls.is_empty(),
        "preflight changed filesystem"
    );
}
#[test]
fn endpoint_allocation_hash_and_private_success() {
    for (address, server) in [(UNIX, "0"), (WINDOWS, "S-1-5-18")] {
        let observer = AllocationObserver::register();
        let f = Fixture::new();
        let mut ops = Ops::new();
        let hash = create_with(
            "agent",
            &f.out,
            address,
            server,
            || Ok(false),
            random,
            &mut ops,
        )
        .unwrap();
        let bytes = f.bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.as_object().unwrap().len() == 3, "three keys");
        assert!(
            v["address"].as_str() == Some(address)
                && v["expected_server_identity"].as_str() == Some(server),
            "endpoint round trip"
        );
        let token = v["token"].as_str().unwrap();
        let mut raw = [0; 32];
        random(&mut raw).unwrap();
        assert!(token == hex::encode(raw), "exact token encoding");
        assert!(
            hash == hex::encode(Sha256::digest(token.as_bytes())),
            "ASCII token hash"
        );
        for wrong in [
            Sha256::digest([0; 64]),
            Sha256::digest([b'0'; 64]),
            Sha256::digest(raw),
        ] {
            assert!(hash != hex::encode(wrong), "wrong hash input");
        }
        let _: crate::local::ClientConfig = serde_json::from_slice(&bytes).unwrap();
        if address == WINDOWS {
            assert!(
                bytes.windows(4).any(|s| s == b"\\\\\\\\"),
                "escaped Windows address"
            );
        }
        f.private();
        assert!(ops.calls == steps(), "operation order");
        observer.check(1, 1);
    }
}
#[test]
fn partial_rng_error_and_unwind_wipe_before_filesystem() {
    for unwind in [false, true] {
        let observer = AllocationObserver::register();
        let f = Fixture::new();
        let mut ops = Ops::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            create_with(
                "agent",
                &f.out,
                UNIX,
                "0",
                || Ok(false),
                |raw| {
                    raw[..7].fill(0xa5);
                    if unwind {
                        panic!("synthetic RNG unwind");
                    }
                    Err(ClientError::Io)
                },
                &mut ops,
            )
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(Err(ClientError::Io))));
        }
        assert!(
            !f.parent.exists() && ops.calls.is_empty(),
            "RNG changed filesystem"
        );
        observer.check(1, 0);
    }
}
#[test]
fn post_create_native_result_matrix_retains_exact_prefix() {
    for (fail, prefix, acquisition) in steps()
        .into_iter()
        .map(|s| (Some(s), false, false))
        .chain([(None, true, false), (None, false, true)])
    {
        let observer = AllocationObserver::register();
        let f = Fixture::new();
        drop(platform::Parent::open(&f.out).unwrap_or_else(|_| panic!("matrix private parent")));
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(unix)]
        let parent_before = fs::metadata(&f.parent).unwrap();
        let mut ops = Ops::new();
        ops.fail = fail;
        ops.prefix = prefix;
        ops.acquisition = acquisition;
        let result = f.run(&mut ops);
        assert!(
            matches!(result, Err(ClientError::Partial)),
            "Partial/no hash"
        );
        let last = if prefix {
            Step::Write
        } else if acquisition {
            Step::Verify
        } else {
            fail.unwrap()
        };
        let order = steps();
        let count = order.iter().position(|s| *s == last).unwrap() + 1;
        assert!(ops.calls == order[..count], "no later operations");
        let bytes = f.bytes();
        if prefix {
            assert!(bytes == b"{\"address\":\"/", "retained prefix");
        } else if last == Step::Write || count == 1 {
            assert!(bytes.is_empty(), "pre-write retained bytes");
        } else {
            assert!(
                serde_json::from_slice::<crate::local::ClientConfig>(&bytes).is_ok(),
                "complete retained JSON"
            );
        }
        f.private();
        #[cfg(unix)]
        {
            let m = fs::metadata(&f.out).unwrap();
            assert!(m.mode() & 0o077 == 0, "private before Mode");
            if last != Step::Mode {
                assert!(m.mode() & 0o777 == 0o600, "exact mode after Mode");
            }
            let after = fs::metadata(&f.parent).unwrap();
            assert!(
                parent_before.mode() == after.mode()
                    && parent_before.uid() == after.uid()
                    && parent_before.gid() == after.gid(),
                "parent mode/owner unchanged"
            );
        }
        assert!(
            fs::read_dir(&f.parent).unwrap().count() == 1,
            "no rollback or extra entry"
        );
        observer.check(1, 1);
    }
}
#[test]
fn existing_target_and_parent_unchanged_and_wiped() {
    let f = Fixture::new();
    let mut ops = Ops::new();
    f.run(&mut ops).unwrap();
    let before = fs::metadata(&f.out).unwrap();
    #[cfg(windows)]
    let security_before = platform::fixture_snapshot(&f.out).unwrap();
    let parent = fs::metadata(&f.parent).unwrap();
    let bytes = f.bytes();
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    assert!(
        matches!(f.run(&mut ops), Err(ClientError::Exists)),
        "exclusive target"
    );
    let after = fs::metadata(&f.out).unwrap();
    #[cfg(windows)]
    assert!(
        security_before == platform::fixture_snapshot(&f.out).unwrap(),
        "existing owner/DACL/facts changed"
    );
    assert!(
        f.bytes() == bytes
            && before.len() == after.len()
            && before.modified().unwrap() == after.modified().unwrap(),
        "existing target changed"
    );
    assert!(
        parent.modified().unwrap() == fs::metadata(&f.parent).unwrap().modified().unwrap(),
        "existing parent changed"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert!(
            before.mode() == after.mode() && before.uid() == after.uid(),
            "existing security changed"
        );
    }
    assert!(ops.calls.is_empty(), "existing target called post-create");
    f.private();
    observer.check(1, 1);
}
#[test]
fn missing_grandparent_refused_without_creation() {
    let f = Fixture::new();
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    let out = f.parent.join("missing").join("credential.json");
    assert!(matches!(
        create_with(
            "agent",
            out.to_str().unwrap(),
            UNIX,
            "0",
            || Ok(false),
            random,
            &mut ops
        ),
        Err(ClientError::Parent)
    ));
    assert!(
        !f.parent.exists() && ops.calls.is_empty(),
        "grandparent creation"
    );
    observer.check(1, 1);
}
#[cfg(unix)]
#[test]
fn unix_parent_privacy_and_symlink_ancestor() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let f = Fixture::new();
    fs::create_dir(&f.parent).unwrap();
    fs::set_permissions(&f.parent, fs::Permissions::from_mode(0o750)).unwrap();
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    assert!(matches!(f.run(&mut ops), Err(ClientError::Parent)));
    assert!(
        fs::metadata(&f.parent).unwrap().permissions().mode() & 0o777 == 0o750,
        "existing parent chmod"
    );
    assert!(!std::path::Path::new(&f.out).exists() && ops.calls.is_empty());
    observer.check(1, 1);
    drop(observer);
    let f = Fixture::new();
    let target = f.parent.with_file_name("target");
    fs::create_dir(&target).unwrap();
    symlink(&target, &f.parent).unwrap();
    let observer = AllocationObserver::register();
    assert!(matches!(f.run(&mut Ops::new()), Err(ClientError::Parent)));
    assert!(fs::read_dir(&target).unwrap().count() == 0);
    observer.check(1, 1);
}
#[cfg(unix)]
#[test]
fn unix_real_invalid_verify_retained_states() {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    struct Invalid {
        ops: Ops,
        kind: u8,
        path: String,
    }
    impl PostCreateOps for Invalid {
        fn mode(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.mode(f)
        }
        fn write(&mut self, f: ClientHandle<'_>, b: &[u8]) -> io::Result<()> {
            self.ops.write(f, b)
        }
        fn flush(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.flush(f)
        }
        fn sync_parent(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.sync_parent(f)
        }
        fn verify(&mut self, f: ClientHandle<'_>, n: u64) -> io::Result<()> {
            match self.kind {
                0 => fs::hard_link(&self.path, format!("{}.link", self.path))?,
                1 => {
                    assert!(
                        unsafe { libc::fchmod(f.as_raw_fd(), 0o640) } == 0,
                        "fixture mode"
                    );
                }
                _ => self.ops.native.write(f, b"x")?,
            }
            self.ops.verify(f, n)
        }
    }
    for kind in 0..3 {
        let observer = AllocationObserver::register();
        let f = Fixture::new();
        let mut ops = Invalid {
            ops: Ops::new(),
            kind,
            path: f.out.clone(),
        };
        assert!(
            matches!(f.run(&mut ops), Err(ClientError::Partial)),
            "invalid Verify/no hash"
        );
        assert!(ops.ops.calls == steps(), "no later operation");
        let bytes = f.bytes();
        let m = fs::metadata(&f.out).unwrap();
        assert!(m.uid() == unsafe { libc::geteuid() }, "fixture owner");
        assert!(
            m.nlink() == if kind == 0 { 2 } else { 1 },
            "retained link count"
        );
        assert!(
            m.mode() & 0o777 == if kind == 1 { 0o640 } else { 0o600 },
            "retained mode"
        );
        if kind == 2 {
            assert!(bytes.last() == Some(&b'x'), "retained suffix");
            assert!(
                serde_json::from_slice::<crate::local::ClientConfig>(&bytes[..bytes.len() - 1])
                    .is_ok()
            );
        } else {
            assert!(serde_json::from_slice::<crate::local::ClientConfig>(&bytes).is_ok());
        }
        assert!(
            fs::read_dir(&f.parent).unwrap().count() == if kind == 0 { 2 } else { 1 },
            "retained names"
        );
        observer.check(1, 1);
    }
}
#[cfg(windows)]
#[test]
fn windows_every_fact_rejection_traverses_verify_to_partial() {
    struct Invalid {
        ops: Ops,
        kind: u8,
    }
    impl PostCreateOps for Invalid {
        fn write(&mut self, f: ClientHandle<'_>, b: &[u8]) -> io::Result<()> {
            self.ops.write(f, b)
        }
        fn flush(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.flush(f)
        }
        fn verify(&mut self, _f: ClientHandle<'_>, n: u64) -> io::Result<()> {
            self.ops.calls.push(Step::Verify);
            let facts = self.ops.native.fixture_facts(self.kind, n);
            self.ops.native.verify_with(|| Ok(facts), n)
        }
    }
    for kind in 0..=10 {
        let observer = AllocationObserver::register();
        let f = Fixture::new();
        let mut ops = Invalid {
            ops: Ops::new(),
            kind,
        };
        let result = f.run(&mut ops);
        if kind == 0 {
            assert!(result.is_ok(), "valid Windows facts control");
        } else {
            assert!(
                matches!(result, Err(ClientError::Partial)),
                "negative Windows fact/no hash"
            );
        }
        assert!(ops.ops.calls == steps(), "fact operation order");
        f.private();
        assert!(
            serde_json::from_slice::<crate::local::ClientConfig>(&f.bytes()).is_ok(),
            "retained complete JSON"
        );
        assert!(
            fs::read_dir(&f.parent).unwrap().count() == 1,
            "no rollback/extra entry"
        );
        observer.check(1, 1);
    }
}
#[test]
fn precreate_io_wipes_and_keeps_existing_parent() {
    let f = Fixture::new();
    f.run(&mut Ops::new()).unwrap();
    let before = fs::metadata(&f.parent).unwrap().modified().unwrap();
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    let long = f.parent.join("a".repeat(300));
    let result = create_with(
        "agent",
        long.to_str().unwrap(),
        UNIX,
        "0",
        || Ok(false),
        random,
        &mut ops,
    );
    assert!(
        matches!(result, Err(ClientError::Io)),
        "native exclusive create error"
    );
    assert!(
        ops.calls.is_empty() && fs::read_dir(&f.parent).unwrap().count() == 1,
        "precreate mutation"
    );
    assert!(
        before == fs::metadata(&f.parent).unwrap().modified().unwrap(),
        "parent changed"
    );
    observer.check(1, 1);
}
#[cfg(windows)]
#[test]
fn windows_nonprivate_parent_and_junction_refused_unchanged() {
    let f = Fixture::new();
    assert!(
        platform::fixture_public_parent(f.parent.to_str().unwrap()).is_ok(),
        "owned public-parent fixture"
    );
    let before = fs::metadata(&f.parent).unwrap().modified().unwrap();
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    assert!(
        matches!(f.run(&mut ops), Err(ClientError::Parent)),
        "foreign grant refused"
    );
    assert!(
        ops.calls.is_empty() && fs::read_dir(&f.parent).unwrap().count() == 0,
        "parent mutated"
    );
    assert!(
        fs::metadata(&f.parent).unwrap().modified().unwrap() == before,
        "parent time changed"
    );
    assert!(
        crate::security::private(&f.parent).is_err(),
        "parent ACL repaired"
    );
    observer.check(1, 1);
    drop(observer);
    let f = Fixture::new();
    let target = f.parent.with_file_name("target");
    fs::create_dir(&target).unwrap();
    assert!(
        platform::fixture_junction(f.parent.to_str().unwrap(), target.to_str().unwrap()).is_ok(),
        "owned junction fixture"
    );
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    assert!(
        matches!(f.run(&mut ops), Err(ClientError::Parent)),
        "junction refused"
    );
    assert!(
        ops.calls.is_empty() && fs::read_dir(&target).unwrap().count() == 0,
        "junction target changed"
    );
    observer.check(1, 1);
    fs::remove_dir(&f.parent).unwrap_or_else(|_| panic!("owned junction cleanup"));
}
#[cfg(windows)]
#[test]
fn windows_native_verify_rejection_and_acquisition_reach_partial() {
    use std::os::windows::{fs::OpenOptionsExt, io::AsHandle};
    let f = Fixture::new();
    f.run(&mut Ops::new()).unwrap();
    let before = f.bytes();
    let len = before.len() as u64;
    let held = fs::File::open(&f.out).unwrap_or_else(|_| panic!("held fixture"));
    let mut native = platform::NativeFileOps::new().unwrap();
    assert!(
        native.verify(held.as_handle(), len).is_ok(),
        "valid native control"
    );
    assert!(
        matches!(
            partial(native.verify(held.as_handle(), len + 1)),
            Err(ClientError::Partial)
        ),
        "native checker rejection"
    );
    let meta = fs::OpenOptions::new()
        .access_mode(windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES)
        .open(&f.out)
        .unwrap_or_else(|_| panic!("attributes-only fixture"));
    assert!(
        matches!(
            partial(native.verify(meta.as_handle(), len)),
            Err(ClientError::Partial)
        ),
        "native acquisition failure"
    );
    drop((held, meta));
    assert!(before == f.bytes(), "verify changed bytes");
    f.private();
}

#[test]
fn held_file_is_noninheritable_and_existing_private_parent_unchanged() {
    struct Inspect {
        ops: Ops,
    }
    impl PostCreateOps for Inspect {
        #[cfg(unix)]
        fn mode(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.mode(f)
        }
        fn write(&mut self, f: ClientHandle<'_>, b: &[u8]) -> io::Result<()> {
            self.ops.write(f, b)
        }
        fn flush(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.flush(f)
        }
        #[cfg(unix)]
        fn sync_parent(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
            self.ops.sync_parent(f)
        }
        fn verify(&mut self, f: ClientHandle<'_>, n: u64) -> io::Result<()> {
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                assert!(
                    unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC != 0,
                    "noninheritable file"
                );
            }
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawHandle;
                let mut flags = 0;
                assert!(
                    unsafe {
                        windows_sys::Win32::Foundation::GetHandleInformation(
                            f.as_raw_handle(),
                            &mut flags,
                        )
                    } != 0,
                    "handle flags acquired"
                );
                assert!(
                    flags & windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT == 0,
                    "noninheritable file"
                );
            }
            self.ops.verify(f, n)
        }
    }
    let f = Fixture::new();
    f.run(&mut Ops::new()).unwrap();
    let before = fs::metadata(&f.parent).unwrap();
    let out = f.parent.join("second.json");
    let mut ops = Inspect { ops: Ops::new() };
    let observer = AllocationObserver::register();
    assert!(
        create_with(
            "agent",
            out.to_str().unwrap(),
            UNIX,
            "0",
            || Ok(false),
            random,
            &mut ops
        )
        .is_ok(),
        "existing private parent accepted"
    );
    let after = fs::metadata(&f.parent).unwrap();
    // Adding a file naturally changes directory mtime; security remains private.
    assert!(
        before.permissions().readonly() == after.permissions().readonly(),
        "parent permissions changed"
    );
    assert!(
        crate::security::private(&f.parent).is_ok(),
        "parent privacy preserved"
    );
    observer.check(1, 1);
}

#[test]
fn preflight_failures_preserve_existing_target_and_parent() {
    let f = Fixture::new();
    f.run(&mut Ops::new()).unwrap();
    let before = fs::metadata(&f.out).unwrap();
    let parent = fs::metadata(&f.parent).unwrap();
    let bytes = f.bytes();
    #[cfg(windows)]
    let security_before = platform::fixture_snapshot(&f.out).unwrap();
    for kind in 0..3 {
        let observer = AllocationObserver::register();
        let mut ops = Ops::new();
        let result = create_with(
            if kind == 0 { "arxd" } else { "agent" },
            &f.out,
            UNIX,
            "0",
            || Ok(kind == 1),
            |raw| {
                raw[0] = 0xa5;
                Err(ClientError::Io)
            },
            &mut ops,
        );
        assert!(
            matches!(
                (kind, result),
                (0, Err(ClientError::Argument))
                    | (1, Err(ClientError::Elevated))
                    | (2, Err(ClientError::Io))
            ),
            "preflight class/no hash"
        );
        assert!(
            ops.calls.is_empty() && f.bytes() == bytes,
            "preflight touched existing target"
        );
        let after = fs::metadata(&f.out).unwrap();
        assert!(
            before.len() == after.len() && before.modified().unwrap() == after.modified().unwrap(),
            "preflight changed file metadata"
        );
        assert!(
            parent.modified().unwrap() == fs::metadata(&f.parent).unwrap().modified().unwrap(),
            "preflight changed parent"
        );
        #[cfg(windows)]
        assert!(
            security_before == platform::fixture_snapshot(&f.out).unwrap(),
            "preflight changed owner/DACL"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert!(
                before.mode() == after.mode() && before.uid() == after.uid(),
                "preflight changed owner/mode"
            );
        }
        observer.check(usize::from(kind == 2), 0);
    }
}

#[test]
fn native_readonly_handle_errors_reach_partial() {
    let f = Fixture::new();
    f.run(&mut Ops::new()).unwrap();
    let before = f.bytes();
    let readonly = fs::File::open(&f.out).unwrap_or_else(|_| panic!("readonly fixture"));
    #[cfg(unix)]
    use std::os::fd::AsFd;
    #[cfg(windows)]
    use std::os::windows::io::AsHandle;
    #[cfg(windows)]
    let handle = readonly.as_handle();
    #[cfg(unix)]
    let handle = readonly.as_fd();
    let mut native = platform::NativeFileOps::new().unwrap();
    assert!(
        matches!(
            partial(native.write(handle, b"synthetic")),
            Err(ClientError::Partial)
        ),
        "native Write failure"
    );
    #[cfg(windows)]
    assert!(
        matches!(partial(native.flush(handle)), Err(ClientError::Partial)),
        "native Flush failure"
    );
    #[cfg(unix)]
    {
        // A pipe is a genuine descriptor on which fsync is invalid; no invalid
        // BorrowedFd or process identity change is needed.
        let (reader, _writer) = std::io::pipe().unwrap();
        assert!(
            matches!(
                partial(native.flush(reader.as_fd())),
                Err(ClientError::Partial)
            ),
            "native Flush failure"
        );
        assert!(
            matches!(
                partial(native.sync_parent(reader.as_fd())),
                Err(ClientError::Partial)
            ),
            "native SyncParent failure"
        );
    }
    drop(readonly);
    assert!(before == f.bytes(), "readonly failure changed bytes");
    f.private();
}

#[cfg(windows)]
#[test]
fn windows_pinned_parent_and_intermediate_refuse_rename_until_release() {
    let f = Fixture::new();
    // Make both components private through the real Parent walker.
    drop(platform::Parent::open(&f.out).unwrap_or_else(|_| panic!("private ancestor")));
    let immediate = f.parent.join("inner");
    let out = immediate.join("credential.json");
    let pins =
        platform::Parent::open(out.to_str().unwrap()).unwrap_or_else(|_| panic!("pinned chain"));
    for path in [&immediate, &f.parent] {
        let moved = path.with_file_name("moved");
        assert!(
            fs::rename(path, &moved).is_err(),
            "pinned directory rename refused"
        );
    }
    assert!(!out.exists(), "pinning created credential");
    drop(pins);
    for path in [&immediate, &f.parent] {
        let moved = path.with_file_name("moved");
        assert!(
            fs::rename(path, &moved).is_ok(),
            "released directory rename succeeds"
        );
        assert!(fs::rename(&moved, path).is_ok(), "owned directory restored");
    }
}

#[cfg(windows)]
#[test]
fn windows_real_deny_and_null_dacl_parent_refused_without_mutation() {
    for null_dacl in [false, true] {
        let f = Fixture::new();
        assert!(
            platform::fixture_negative_parent(f.parent.to_str().unwrap(), null_dacl).is_ok(),
            "negative ACL fixture"
        );
        let before = platform::fixture_snapshot(f.parent.to_str().unwrap()).unwrap();
        let time = fs::metadata(&f.parent).unwrap().modified().unwrap();
        let observer = AllocationObserver::register();
        let mut ops = Ops::new();
        assert!(
            matches!(f.run(&mut ops), Err(ClientError::Parent)),
            "real ACL refused/no hash"
        );
        assert!(
            ops.calls.is_empty() && !std::path::Path::new(&f.out).exists(),
            "no post-create or target"
        );
        assert!(
            fs::read_dir(&f.parent).unwrap().count() == 0,
            "no directory mutation"
        );
        assert!(
            before == platform::fixture_snapshot(f.parent.to_str().unwrap()).unwrap(),
            "ACL facts unchanged"
        );
        assert!(
            time == fs::metadata(&f.parent).unwrap().modified().unwrap(),
            "parent time unchanged"
        );
        observer.check(1, 1);
    }
}

#[test]
fn intermediate_link_ancestor_refused_without_mutation() {
    let f = Fixture::new();
    let target = f.parent.with_file_name("target");
    fs::create_dir(&target).unwrap();
    let inner = target.join("inner");
    fs::create_dir(&inner).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::set_permissions(&inner, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&target, &f.parent).unwrap();
    }
    #[cfg(windows)]
    {
        // Give the final parent the real private descriptor so only the
        // intermediate reparse check can refuse this chain.
        fs::remove_dir(&inner).unwrap();
        let dummy = inner.join("unused.json");
        drop(
            platform::Parent::open(dummy.to_str().unwrap())
                .unwrap_or_else(|_| panic!("private final parent")),
        );
        assert!(
            platform::fixture_junction(f.parent.to_str().unwrap(), target.to_str().unwrap())
                .is_ok(),
            "intermediate junction fixture"
        );
    }
    let out = f.parent.join("inner").join("credential.json");
    let observer = AllocationObserver::register();
    let mut ops = Ops::new();
    assert!(
        matches!(
            create_with(
                "agent",
                out.to_str().unwrap(),
                UNIX,
                "0",
                || Ok(false),
                random,
                &mut ops
            ),
            Err(ClientError::Parent)
        ),
        "intermediate link refused/no hash"
    );
    assert!(
        ops.calls.is_empty() && fs::read_dir(&inner).unwrap().count() == 0,
        "link target unchanged"
    );
    observer.check(1, 1);
    #[cfg(windows)]
    fs::remove_dir(&f.parent).unwrap_or_else(|_| panic!("owned junction cleanup"));
}

#[cfg(unix)]
#[test]
fn unix_identity_mismatch_is_pure() {
    for (uid, euid, gid, egid, expected) in [
        (1000, 1000, 1000, 1000, false),
        (0, 0, 1000, 1000, true),
        (0, 1000, 1000, 1000, true),
        (1000, 0, 1000, 1000, true),
        (1000, 1001, 1000, 1000, true),
        (1000, 1000, 1000, 1001, true),
    ] {
        assert!(
            platform::elevated_ids(uid, euid, gid, egid) == expected,
            "UID/GID refusal"
        );
    }
}
