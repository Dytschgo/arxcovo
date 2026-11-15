//! Fixed disposable candidate; native authority requires the separate D7i gates.
use super::*;

// Fixed test-only delivery; no environment/config selection of policy facts.
struct Prepared {
    accounts: [OwnerConfig; 2], // rule, then no-rule; driver-owned and pinned
    arxd: PathBuf,
    reentry: PathBuf,
}
fn prepared_delivery() -> Option<Prepared> {
    let (prepared, _held) = read_fixed_delivery(A7_PROFILE)?;
    let _runtime = runtime_package()?;
    let run =
        std::env::var("GITHUB_RUN_ID").ok()? + "-" + &std::env::var("GITHUB_RUN_ATTEMPT").ok()?;
    let _view = controlled_view(&run, &prepared.accounts)?;
    Some(prepared)
}

// SHA256 of canonical reviewed package/default/policy/five-view facts (evidence
// JSON, ADR0020 D9c). A label only: never host qualification or a live check.
const A7_PROFILE: &str = "99e7f5b4b3b63ff13955432bf9b7cde0436c9ab57afbc93fbe99bb45eb17017e";
const PACKAGE: [(&str, u64, &str); 3] = [
    (
        "/usr/bin/sudo",
        282032,
        "6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff",
    ),
    (
        "/usr/libexec/sudo/sudoers.so",
        588080,
        "151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c",
    ),
    (
        "/usr/libexec/sudo/libsudo_util.so.0.0.0",
        133944,
        "aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d",
    ),
];
fn package_file(path: &Path, size: u64, digest: &str) -> Option<std::fs::File> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let file = delivery_open(path, false)?;
    let before = file.metadata().ok()?;
    if !before.is_file()
        || before.uid() != 0
        || before.nlink() != 1
        || before.mode() & 0o022 != 0
        || before.len() != size
    {
        return None;
    }
    let bytes = controlled_read(&file, size)?;
    if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(&bytes)) != digest {
        return None;
    }
    let again = delivery_open(path, false)?;
    let snapshot = |m: &std::fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.gid(),
            m.nlink(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    if snapshot(&before) != snapshot(&file.metadata().ok()?)
        || snapshot(&before) != snapshot(&again.metadata().ok()?)
    {
        return None;
    }
    Some(file)
}
fn runtime_link(parent: &std::fs::File, name: &std::ffi::CStr, targets: &[&[u8]]) -> Option<()> {
    use std::os::fd::AsRawFd;
    let read = || {
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return None;
        }
        let st = unsafe { st.assume_init() };
        (st.st_mode & libc::S_IFMT == libc::S_IFLNK && st.st_uid == 0 && st.st_nlink == 1)
            .then_some((st.st_dev, st.st_ino, st.st_size, st.st_mtime, st.st_ctime))
    };
    let before = read()?;
    let mut bytes = [0u8; 32];
    let n = unsafe {
        libc::readlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if n < 0
        || n as usize >= bytes.len()
        || !targets.contains(&&bytes[..n as usize])
        || before.2 != n as i64
        || read()? != before
    {
        return None;
    }
    Some(())
}
fn runtime_package() -> Option<Vec<std::fs::File>> {
    use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
    if std::env::consts::ARCH != "x86_64"
        || unsafe { libc::getuid() } != 0
        || unsafe { libc::geteuid() } != 0
    {
        return None;
    }
    let mut held = Vec::new();
    for (path, size, digest) in PACKAGE {
        held.push(package_file(Path::new(path), size, digest)?);
    }
    let util = delivery_open(Path::new("/usr/libexec/sudo"), true)?;
    for name in [c"libsudo_util.so", c"libsudo_util.so.0"] {
        runtime_link(&util, name, &[b"libsudo_util.so.0.0.0"])?;
    }
    let root = delivery_open(Path::new("/"), true)?;
    runtime_link(&root, c"bin", &[b"usr/bin", b"/usr/bin"])?;
    let usrbin = delivery_open(Path::new("/usr/bin"), true)?;
    let mut alias = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstatat(root.as_raw_fd(), c"bin".as_ptr(), alias.as_mut_ptr(), 0) } != 0 {
        return None;
    }
    let alias = unsafe { alias.assume_init() };
    let target = usrbin.metadata().ok()?;
    if alias.st_dev != target.dev() || alias.st_ino != target.ino() {
        return None;
    }
    // D9c: inside the unit /usr/local/bin is the closed empty view, so C2 must be
    // absent (ENOENT); presence or any other error refuses. controlled_view proves
    // the view itself; host C2 is never consulted or claimed.
    let local = delivery_open(Path::new("/usr/local/bin"), true)?;
    if view_c2(&local) != C2::Absent {
        return None;
    }
    held.extend([util, root, usrbin, local]);
    Some(held)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    run: String,
    source_sha: String,
    host_profile_sha256: String,
    accounts: [DeliveryAccount; 2],
    artifacts: [DeliveryArtifact; 2],
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryAccount {
    uid: u32,
    gid: u32,
    base_dev: u64,
    base_ino: u64,
    workspace_dev: u64,
    workspace_ino: u64,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryArtifact {
    size: u64,
    sha256: String,
    dev: u64,
    ino: u64,
}
#[derive(Clone, Copy, Debug)]
struct DeliveryIdentity {
    size: u64,
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    nlink: u64,
}
fn artifact_identity(a: &DeliveryArtifact, m: DeliveryIdentity) -> bool {
    m.size == a.size
        && m.dev == a.dev
        && m.ino == a.ino
        && m.mode == 0o100755
        && m.uid == 0
        && m.nlink == 1
}
fn account_record(a: &DeliveryAccount, uid: u32, gid: u32, shell: &[u8]) -> bool {
    uid == a.uid && gid == a.gid && shell == b"/usr/sbin/nologin"
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn delivery_names(run: &str) -> Option<[String; 2]> {
    let (id, attempt) = run.split_once('-')?;
    let decimal = |s: &str, max| {
        !s.is_empty()
            && s.len() <= max
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
    };
    if !decimal(id, 20) || !decimal(attempt, 6) {
        return None;
    }
    let names = [
        format!("arxsn{id}x{attempt}r"),
        format!("arxsn{id}x{attempt}n"),
    ];
    names.iter().all(|s| s.len() <= 32).then_some(names)
}
fn delivery_schema(
    bytes: &[u8],
    run: &str,
    source: &str,
    workflow: &str,
    reviewed: &str,
    profile: &str,
) -> Option<Delivery> {
    if bytes.len() > 4096
        || !hex(source, 40)
        || source != workflow
        || source != reviewed
        || !hex(profile, 64)
    {
        return None;
    }
    // Struct deserialization rejects duplicates as well as missing/unknown fields;
    // from_slice requires EOF after the single record (apart from whitespace).
    let d: Delivery = serde_json::from_slice(bytes).ok()?;
    delivery_names(run)?;
    if d.run != run
        || d.source_sha != source
        || d.host_profile_sha256 != profile
        || d.accounts.iter().any(|a| a.uid < 1000 || a.gid < 1000)
        || d.accounts[0].uid == d.accounts[1].uid
        || d.accounts[0].gid == d.accounts[1].gid
        || d.artifacts
            .iter()
            .any(|a| a.size < 4 || a.size > 128 * 1024 * 1024 || !hex(&a.sha256, 64))
    {
        return None;
    }
    Some(d)
}
fn delivery_stage(exe: &Path, run: &str) -> Option<PathBuf> {
    delivery_names(run)?;
    let stage = PathBuf::from(format!("/var/lib/arx-ci-{run}"));
    let name = exe.file_name()?.to_str()?.strip_prefix("arxd-")?;
    (!name.is_empty()
        && hex(name, name.len())
        && exe.parent()? == stage.join("source/target/debug/deps"))
    .then_some(stage)
}

// Every component is opened root-down, relative to the held parent; never
// canonicalize away a link. Only root-writable ancestry makes path-based exec
// after the immediate recheck safe: build accounts have no ancestor write bit.
fn delivery_open(path: &Path, directory: bool) -> Option<std::fs::File> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    };
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    let root = unsafe { libc::open(c"/".as_ptr(), flags | libc::O_DIRECTORY) };
    if root < 0 {
        return None;
    }
    let mut held = unsafe { std::fs::File::from_raw_fd(root) };
    let parts: Vec<_> = path.components().collect();
    if parts.first() != Some(&std::path::Component::RootDir) {
        return None;
    }
    for (index, part) in parts.iter().enumerate().skip(1) {
        let parent = held.metadata().ok()?;
        if !parent.is_dir() || parent.uid() != 0 || parent.mode() & 0o022 != 0 {
            return None;
        }
        let std::path::Component::Normal(name) = part else {
            return None;
        };
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes()).ok()?;
        let dir = directory || index + 1 < parts.len();
        let fd = unsafe {
            libc::openat(
                held.as_raw_fd(),
                name.as_ptr(),
                flags | if dir { libc::O_DIRECTORY } else { 0 },
            )
        };
        if fd < 0 {
            return None;
        }
        held = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    let info = held.metadata().ok()?;
    if directory && (!info.is_dir() || info.uid() != 0 || info.mode() & 0o022 != 0) {
        return None;
    }
    Some(held)
}
fn file_identity(file: &std::fs::File) -> Option<DeliveryIdentity> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().ok()?;
    Some(DeliveryIdentity {
        size: m.len(),
        dev: m.dev(),
        ino: m.ino(),
        mode: m.mode(),
        uid: m.uid(),
        nlink: m.nlink(),
    })
}
fn delivery_account(name: &str, a: &DeliveryAccount) -> Option<()> {
    use std::mem::MaybeUninit;
    let name = std::ffi::CString::new(name).ok()?;
    let mut buffer = vec![0u8; 65536];
    let mut pw = MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            pw.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() {
        return None;
    }
    let pw = unsafe { pw.assume_init() };
    // NSS strings must lie within the fixed lookup buffer; never unbounded CStr.
    let offset = (pw.pw_shell as usize).checked_sub(buffer.as_ptr() as usize)?;
    let shell = buffer.get(offset..)?;
    let end = shell.iter().position(|b| *b == 0)?;
    if !account_record(a, pw.pw_uid, pw.pw_gid, &shell[..end]) {
        return None;
    }
    let mut groups = [0; 64];
    let mut count = 64;
    let rc = unsafe { libc::getgrouplist(name.as_ptr(), a.gid, groups.as_mut_ptr(), &mut count) };
    if rc < 0 || count != 1 || groups[0] != a.gid {
        return None;
    }
    let mut gr = MaybeUninit::<libc::group>::uninit();
    let mut found = std::ptr::null_mut();
    let rc = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            gr.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() {
        return None;
    }
    let gr = unsafe { gr.assume_init() };
    if gr.gr_gid != a.gid {
        return None;
    }
    let offset = (gr.gr_mem as usize).checked_sub(buffer.as_ptr() as usize)?;
    buffer.get(offset..offset.checked_add(std::mem::size_of::<*mut libc::c_char>())?)?;
    if !unsafe { gr.gr_mem.read_unaligned() }.is_null() {
        return None;
    }
    Some(())
}
fn delivery_workspace(
    path: &Path,
    dev: u64,
    ino: u64,
    uid: u32,
    gid: u32,
) -> Option<std::fs::File> {
    // The account owns the last two components, so open them below the protected
    // /var/lib descriptor, then compare both pinned identities without traversal.
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    };
    let parent = delivery_open(Path::new("/var/lib"), true)?;
    let relative = path.strip_prefix("/var/lib").ok()?;
    let mut held = parent;
    for part in relative.components() {
        let std::path::Component::Normal(name) = part else {
            return None;
        };
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes()).ok()?;
        let fd = unsafe {
            libc::openat(
                held.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }
        held = unsafe { std::fs::File::from_raw_fd(fd) };
        let m = held.metadata().ok()?;
        if m.uid() != uid || m.gid() != gid || m.mode() & 0o7777 != 0o700 {
            return None;
        }
    }
    let m = held.metadata().ok()?;
    (m.dev() == dev && m.ino() == ino).then_some(held)
}
fn read_fixed_delivery(profile: &str) -> Option<(Prepared, Vec<std::fs::File>)> {
    use sha2::{Digest, Sha256};
    use std::{io::Read, os::unix::fs::MetadataExt};
    let env = |name| std::env::var(name).ok();
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Linux"),
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_REPOSITORY", "Dytschgo/arxcovo"),
        ("GITHUB_ACTOR", "Dytschgo"),
        ("GITHUB_TRIGGERING_ACTOR", "Dytschgo"),
        ("GITHUB_REF", "refs/heads/main"),
    ] {
        if env(key)?.as_str() != value {
            return None;
        }
    }
    if env("GITHUB_RUN_ATTEMPT")?.as_str() != "1" {
        return None;
    }
    let run = format!("{}-{}", env("GITHUB_RUN_ID")?, env("GITHUB_RUN_ATTEMPT")?);
    let stage = delivery_stage(&std::env::current_exe().ok()?, &run)?;
    if delivery_open(&stage, true)?.metadata().ok()?.mode() & 0o7777 != 0o700 {
        return None;
    }
    let source = env("GITHUB_SHA")?;
    let mut file = delivery_open(&stage.join("a7-a11-delivery.json"), false)?;
    let m = file.metadata().ok()?;
    if !m.is_file()
        || m.uid() != 0
        || m.nlink() != 1
        || m.mode() & 0o7777 != 0o600
        || m.len() > 4096
    {
        return None;
    }
    let mut bytes = Vec::new();
    file.by_ref().take(4097).read_to_end(&mut bytes).ok()?;
    let d = delivery_schema(
        &bytes,
        &run,
        &source,
        &env("ARX_WORKFLOW_SHA")?,
        &env("ARX_REVIEWED_SHA")?,
        profile,
    )?;
    let names = delivery_names(&run)?;
    let mut held = vec![file];
    let mut accounts = Vec::new();
    for (a, name) in d.accounts.iter().zip(names) {
        delivery_account(&name, a)?;
        let base = PathBuf::from(format!("/var/lib/{name}"));
        let workspace = base.join("workspace");
        held.push(delivery_workspace(
            &base, a.base_dev, a.base_ino, a.uid, a.gid,
        )?);
        held.push(delivery_workspace(
            &workspace,
            a.workspace_dev,
            a.workspace_ino,
            a.uid,
            a.gid,
        )?);
        accounts.push(OwnerConfig {
            account: name,
            uid: a.uid,
            gid: a.gid,
            workspace,
            protected_directories: vec![stage.clone()],
            environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            admin: false,
            cgroup_parent: Some("/sys/fs/cgroup/system.slice/arxd.service/runs".into()),
            memory_bytes: 64 * 1024 * 1024,
            pids: 32,
        });
    }
    let directory = PathBuf::from(format!("/usr/libexec/arxcovo-a7-a11-{run}"));
    let dir = delivery_open(&directory, true)?;
    if dir.metadata().ok()?.mode() & 0o7777 != 0o755 {
        return None;
    }
    held.push(dir);
    for (a, name) in d.artifacts.iter().zip(["arxd", "reentry"]) {
        let mut file = delivery_open(&directory.join(name), false)?;
        if !artifact_identity(a, file_identity(&file)?) {
            return None;
        }
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).ok()?;
            if n == 0 {
                break;
            }
            if count == 0 && !buffer[..n].starts_with(b"\x7fELF") {
                return None;
            }
            count += n as u64;
            if count > a.size {
                return None;
            }
            hash.update(&buffer[..n]);
        }
        if count != a.size || format!("{:x}", hash.finalize()) != a.sha256 {
            return None;
        }
        held.push(file);
    }
    // The controller is the original libtest from the SINGLE test build. Bind
    // reentry to those same harness bytes independently of the bin build.
    let mut harness = delivery_open(&std::env::current_exe().ok()?, false)?;
    let m = harness.metadata().ok()?;
    if !m.is_file()
        || m.uid() != 0
        || m.nlink() != 1
        || m.mode() & 0o022 != 0
        || m.len() != d.artifacts[1].size
    {
        return None;
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = harness.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > d.artifacts[1].size {
            return None;
        }
        hash.update(&buffer[..n]);
    }
    if count != d.artifacts[1].size || format!("{:x}", hash.finalize()) != d.artifacts[1].sha256 {
        return None;
    }
    held.push(harness);
    // Hashing is bounded but takes time. Reopen both names with NOFOLLOW and
    // CLOEXEC and fstat every pinned scalar immediately before admission returns.
    for (a, name) in d.artifacts.iter().zip(["arxd", "reentry"]) {
        let file = delivery_open(&directory.join(name), false)?;
        if !artifact_identity(a, file_identity(&file)?) {
            return None;
        }
        held.push(file);
    }
    for (a, config) in d.accounts.iter().zip(&accounts) {
        held.push(delivery_workspace(
            config.workspace.parent()?,
            a.base_dev,
            a.base_ino,
            a.uid,
            a.gid,
        )?);
        held.push(delivery_workspace(
            &config.workspace,
            a.workspace_dev,
            a.workspace_ino,
            a.uid,
            a.gid,
        )?);
    }
    Some((
        Prepared {
            accounts: accounts.try_into().ok()?,
            arxd: directory.join("arxd"),
            reentry: directory.join("reentry"),
        },
        held,
    ))
}

// D9b fixed candidate; package equality plus this live view binds reviewed defaults.
const CONTROLLED_PLUGIN: &str = "/usr/libexec/sudo/sudoers.so";
fn controlled_literals(run: &str) -> Option<Vec<(&'static str, Vec<u8>)>> {
    let names = delivery_names(run)?;
    Some(vec![
        ("sudoers", b"#includedir /etc/sudoers.d\n".to_vec()),
        (
            "sudo.conf",
            format!("Plugin sudoers_policy {CONTROLLED_PLUGIN}\n").into_bytes(),
        ),
        (
            "nsswitch.conf",
            b"passwd: files\ngroup: files\nsudoers: files\n".to_vec(),
        ),
        (
            "sudoers.d/10-a7-case0",
            format!("{} ALL=(root) NOPASSWD: /usr/bin/true\n", names[0]).into_bytes(),
        ),
    ])
}
// ADR0020 D9c: exactly the four D9b policy views plus the closed empty C2 view.
const CONTROLLED_VIEWS: [(&str, &str); 5] = [
    ("sudoers", "/etc/sudoers"),
    ("sudo.conf", "/etc/sudo.conf"),
    ("nsswitch.conf", "/etc/nsswitch.conf"),
    ("sudoers.d", "/etc/sudoers.d"),
    ("c2-empty", "/usr/local/bin"),
];
fn controlled_mappings(run: &str, mappings: &[&str]) -> bool {
    if delivery_names(run).is_none() {
        return false;
    }
    let expected: Vec<_> = CONTROLLED_VIEWS
        .iter()
        .map(|(n, target)| format!("/var/lib/arx-ci-{run}/fx/{n}:{target}:rbind"))
        .collect();
    mappings.len() == 5
        && expected
            .iter()
            .all(|e| mappings.iter().filter(|m| **m == e).count() == 1)
}
fn controlled_mounts(text: &str) -> bool {
    if text.len() > 1024 * 1024 {
        return false;
    }
    let mut seen = [0; 5];
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 10 || !fields.contains(&"-") {
            return false;
        }
        let path = fields[4];
        if let Some(i) = CONTROLLED_VIEWS.iter().position(|(_, p)| *p == path) {
            if !fields[5].split(',').any(|o| o == "ro") {
                return false;
            }
            seen[i] += 1;
        } else if path.starts_with("/etc/") || path.starts_with("/usr/local/bin/") {
            // No other /etc mount and no submount below the closed C2 view.
            return false;
        }
    }
    seen == [1; 5]
}
fn controlled_bytes(run: &str, entries: &[(&str, &[u8])]) -> bool {
    let Some(expected) = controlled_literals(run) else {
        return false;
    };
    entries.len() == 4
        && expected.iter().all(|(n, b)| {
            entries
                .iter()
                .filter(|(name, data)| name == n && *data == b)
                .count()
                == 1
        })
}
// D9c closed empty C2 view facts, observed from held descriptors inside the unit.
#[derive(Clone, Copy, PartialEq, Debug)]
struct ViewNode {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum C2 {
    Absent,
    Present,
    Unknown,
}
struct EmptyView<'a> {
    source: [ViewNode; 2], // held fx/c2-empty, then its fresh NOFOLLOW reopen
    view: [ViewNode; 2],   // held /usr/local/bin, then its fresh NOFOLLOW reopen
    source_entries: &'a [String],
    view_entries: &'a [String],
    readonly: bool, // fstatvfs ST_RDONLY on the held view
    c2: [C2; 2],    // fstatat(sudo, NOFOLLOW) on the held and reopened view
}
fn empty_view(f: &EmptyView) -> bool {
    // Exactly root:root directory 0755. The real low-UID probe opens the C2
    // directory O_RDONLY|O_DIRECTORY: 0700 would give EACCES (unknown, refused)
    // instead of sudo ENOENT, and any group/other write bit is refused.
    let node = |n: &ViewNode| n.uid == 0 && n.gid == 0 && n.mode == 0o040755;
    // A bind keeps st_dev/st_ino: every view and reopen IS the owned source inode.
    let same = |n: &ViewNode| n.dev == f.source[0].dev && n.ino == f.source[0].ino;
    f.source.iter().chain(f.view.iter()).all(node)
        && f.source.iter().chain(f.view.iter()).all(same)
        && f.source_entries.is_empty()
        && f.view_entries.is_empty()
        && f.readonly
        && f.c2 == [C2::Absent; 2]
}
fn controlled_read(file: &std::fs::File, bound: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(bound + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= bound).then_some(bytes)
}
// Held-descriptor C2 lookup without following links; only ENOENT is absence.
fn view_c2(directory: &std::fs::File) -> C2 {
    use std::os::fd::AsRawFd;
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    let rc = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            c"sudo".as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc == 0 {
        C2::Present
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        C2::Absent
    } else {
        C2::Unknown
    }
}
// Bounded entry names of a held directory; never follows entries.
fn held_entries(directory: &std::fs::File, bound: usize) -> Option<Vec<String>> {
    use std::os::fd::AsRawFd;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .ok()?
        .take(bound)
    {
        entries.push(entry.ok()?.file_name().into_string().ok()?);
    }
    Some(entries)
}
fn controlled_unit_mappings() -> Option<String> {
    use std::{
        io::Read,
        os::fd::AsRawFd,
        process::{Command, Stdio},
    };
    let mut child = Command::new("/usr/bin/systemctl")
        .args([
            "show",
            "arxd.service",
            "--property=BindReadOnlyPaths",
            "--value",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let result = (|| {
        let mut pipe = child.stdout.take()?;
        let fd = pipe.as_raw_fd();
        if unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
            return None;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > 4096 {
                        return None;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return None,
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        loop {
            if let Some(status) = child.try_wait().ok()? {
                return status
                    .success()
                    .then(|| String::from_utf8(bytes).ok())
                    .flatten();
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
fn controlled_view(run: &str, accounts: &[OwnerConfig; 2]) -> Option<Vec<std::fs::File>> {
    use std::{
        mem::MaybeUninit,
        os::{fd::AsRawFd, unix::fs::MetadataExt},
    };
    let mappings = controlled_unit_mappings()?;
    if !controlled_mappings(run, &mappings.split_whitespace().collect::<Vec<_>>()) {
        return None;
    }
    // Fixed kernel path only, bounded; never called by portable pure tests.
    let mounts = controlled_read(
        &std::fs::File::open("/proc/self/mountinfo").ok()?,
        1024 * 1024,
    )?;
    if !controlled_mounts(std::str::from_utf8(&mounts).ok()?) {
        return None;
    }
    let mut observed = Vec::new();
    let stage = PathBuf::from(format!("/var/lib/arx-ci-{run}/fx"));
    let mut held = Vec::new();
    for name in [
        "",
        "sudoers.d",
        "sudoers",
        "sudo.conf",
        "nsswitch.conf",
        "sudoers.d/10-a7-case0",
    ] {
        let directory = name.is_empty() || name == "sudoers.d";
        let source = delivery_open(&stage.join(name), directory)?;
        let view = delivery_open(&Path::new("/etc").join(name), directory)?;
        let a = source.metadata().ok()?;
        let b = view.metadata().ok()?;
        let valid = |m: &std::fs::Metadata| {
            m.uid() == 0
                && m.gid() == 0
                && if directory {
                    m.is_dir() && m.mode() & 0o7777 == 0o700
                } else {
                    m.is_file() && m.mode() == 0o100440 && m.nlink() == 1
                }
        };
        // fx itself maps no /etc directory. Validate its protected staging only.
        if name.is_empty() {
            if !valid(&a) {
                return None;
            }
        } else {
            if !valid(&a) || !valid(&b) || a.dev() != b.dev() || a.ino() != b.ino() {
                return None;
            }
            let mut fs = MaybeUninit::<libc::statvfs>::uninit();
            if unsafe { libc::fstatvfs(view.as_raw_fd(), fs.as_mut_ptr()) } != 0
                || unsafe { fs.assume_init() }.f_flag & libc::ST_RDONLY == 0
            {
                return None;
            }
        }
        if directory {
            let expected: Vec<&str> = if name.is_empty() {
                vec![
                    "sudoers",
                    "sudo.conf",
                    "nsswitch.conf",
                    "sudoers.d",
                    "c2-empty",
                ]
            } else {
                vec!["10-a7-case0"]
            };
            // Held descriptor path, not an arbitrary caller path; no symlink traversal of entries.
            for fd in if name.is_empty() {
                vec![source.as_raw_fd()]
            } else {
                vec![source.as_raw_fd(), view.as_raw_fd()]
            } {
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(format!("/proc/self/fd/{fd}"))
                    .ok()?
                    .take(6)
                {
                    entries.push(entry.ok()?.file_name().into_string().ok()?);
                }
                if entries.len() != expected.len()
                    || !expected.iter().all(|e| entries.iter().any(|n| n == e))
                {
                    return None;
                }
            }
        } else {
            let source_bytes = controlled_read(&source, 4096)?;
            let view_bytes = controlled_read(&view, 4096)?;
            if source_bytes != view_bytes {
                return None;
            }
            observed.push((name, view_bytes));
        }
        let again = delivery_open(&stage.join(name), directory)?
            .metadata()
            .ok()?;
        if again.dev() != a.dev() || again.ino() != a.ino() {
            return None;
        }
        if !name.is_empty() {
            let again_view = delivery_open(&Path::new("/etc").join(name), directory)?
                .metadata()
                .ok()?;
            if !valid(&again_view) || again_view.dev() != b.dev() || again_view.ino() != b.ino() {
                return None;
            }
        }
        if !valid(&again) {
            return None;
        }
        held.extend([source, view]);
    }
    if !controlled_bytes(
        run,
        &observed
            .iter()
            .map(|(n, b)| (*n, b.as_slice()))
            .collect::<Vec<_>>(),
    ) {
        return None;
    }
    // D9c fifth view: the owned empty source IS the in-unit /usr/local/bin.
    // Configuration text is not proof; held and reopened descriptors are.
    let source = delivery_open(&stage.join("c2-empty"), true)?;
    let view = delivery_open(Path::new("/usr/local/bin"), true)?;
    let node = |file: &std::fs::File| {
        let m = file.metadata().ok()?;
        Some(ViewNode {
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
        })
    };
    let source_entries = held_entries(&source, 2)?;
    let view_entries = held_entries(&view, 2)?;
    let mut fs = MaybeUninit::<libc::statvfs>::uninit();
    let readonly = unsafe { libc::fstatvfs(view.as_raw_fd(), fs.as_mut_ptr()) } == 0
        && unsafe { fs.assume_init() }.f_flag & libc::ST_RDONLY != 0;
    let held_c2 = view_c2(&view);
    let source_again = delivery_open(&stage.join("c2-empty"), true)?;
    let view_again = delivery_open(Path::new("/usr/local/bin"), true)?;
    if !empty_view(&EmptyView {
        source: [node(&source)?, node(&source_again)?],
        view: [node(&view)?, node(&view_again)?],
        source_entries: &source_entries,
        view_entries: &view_entries,
        readonly,
        c2: [held_c2, view_c2(&view_again)],
    }) {
        return None;
    }
    held.extend([source, view, source_again, view_again]);
    for a in accounts {
        let pin = DeliveryAccount {
            uid: a.uid,
            gid: a.gid,
            base_dev: 0,
            base_ino: 0,
            workspace_dev: 0,
            workspace_ino: 0,
        };
        delivery_account(&a.account, &pin)?;
    }
    Some(held)
}

#[test]
fn a7_controlled_policy_discriminators() {
    let run = "123-1";
    let literals = controlled_literals(run).unwrap();
    let entries: Vec<_> = literals.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    assert!(controlled_bytes(run, &entries));
    for i in 0..4 {
        let mut bad = entries.clone();
        bad.remove(i);
        assert!(!controlled_bytes(run, &bad));
        let mut bad = entries.clone();
        bad[i].1 = b"arxsn123x1n ALL=(root) NOPASSWD: /usr/bin/true\n";
        assert!(!controlled_bytes(run, &bad));
    }
    let mut bad = entries.clone();
    bad.push(("sudoers.d/extra", b"ALL ALL=(ALL) ALL\n"));
    assert!(!controlled_bytes(run, &bad));
    let mut mappings: Vec<_> = ["sudoers", "sudo.conf", "nsswitch.conf", "sudoers.d"]
        .iter()
        .map(|n| format!("/var/lib/arx-ci-{run}/fx/{n}:/etc/{n}:rbind"))
        .collect();
    mappings.push(format!(
        "/var/lib/arx-ci-{run}/fx/c2-empty:/usr/local/bin:rbind"
    ));
    let refs: Vec<_> = mappings.iter().map(String::as_str).collect();
    assert!(controlled_mappings(run, &refs));
    // The D9b four-set alone (missing C2 view) and every other subset refuse.
    for i in 0..5 {
        let mut missing = refs.clone();
        missing.remove(i);
        assert!(!controlled_mappings(run, &missing));
        let mut duplicate = missing.clone();
        duplicate.push(refs[(i + 1) % 5]);
        assert!(!controlled_mappings(run, &duplicate));
    }
    let mut bad = refs.clone();
    bad.push(refs[0]);
    assert!(!controlled_mappings(run, &bad));
    let mut extra = refs.clone();
    extra.push("/foreign:/etc/other:rbind");
    assert!(!controlled_mappings(run, &extra));
    for i in [0, 4] {
        // Optional missing-source prefix is never admitted, including the C2 view.
        let mut strings = mappings.clone();
        strings[i].insert(0, '-');
        assert!(!controlled_mappings(
            run,
            &strings.iter().map(String::as_str).collect::<Vec<_>>()
        ));
    }
    for wrong in [
        "/var/lib/arx-ci-123-1/fx/c2-empty:/usr/local:rbind",
        "/var/lib/arx-ci-123-1/fx/c2-empty:/usr/local/bin:norbind",
        "/var/lib/arx-ci-123-1/fx/sudoers.d:/usr/local/bin:rbind",
        "/usr/local/bin:/usr/local/bin:rbind",
    ] {
        let mut swapped = refs.clone();
        swapped[4] = wrong;
        assert!(!controlled_mappings(run, &swapped));
    }
    let mut mounts: String = ["sudoers", "sudo.conf", "nsswitch.conf", "sudoers.d"]
        .iter()
        .map(|n| format!("1 2 0:1 /fx/{n} /etc/{n} ro,nosuid - ext4 /dev/x rw\n"))
        .collect();
    let view = "1 2 0:1 /fx/c2-empty /usr/local/bin ro,nosuid - ext4 /dev/x rw\n";
    // ProtectSystem's own /usr and /etc lines are not extra policy mounts.
    let protect = "3 1 0:1 /usr /usr ro - ext4 /dev/x rw\n3 1 0:1 /etc /etc ro - ext4 /dev/x rw\n";
    assert!(!controlled_mounts(&mounts));
    mounts = protect.to_string() + &mounts + view;
    assert!(controlled_mounts(&mounts));
    assert!(!controlled_mounts(&mounts.replacen(
        "ro,nosuid",
        "rw,nosuid",
        1
    )));
    assert!(!controlled_mounts(&mounts.replace(
        "/usr/local/bin ro,nosuid",
        "/usr/local/bin rw,nosuid"
    )));
    assert!(!controlled_mounts(
        &mounts.lines().skip(3).collect::<Vec<_>>().join("\n")
    ));
    assert!(!controlled_mounts(
        &(mounts.clone() + "1 2 0:1 /x /etc/other ro - ext4 /dev/x rw\n")
    ));
    assert!(!controlled_mounts(
        &(mounts.clone() + mounts.lines().nth(2).unwrap())
    ));
    // Stacked view, submount below the view, and a missing view all refuse.
    assert!(!controlled_mounts(&(mounts.clone() + view)));
    assert!(!controlled_mounts(
        &(mounts.clone() + "1 2 0:1 /x /usr/local/bin/sudo ro - ext4 /dev/x rw\n")
    ));
    assert!(!controlled_mounts(&mounts.replace(view, "")));
    assert!(!controlled_mappings("124-1", &refs));
}

#[test]
fn a7_c2_empty_view_discriminators() {
    // Observed facts: [source, source reopen, view, view reopen], then emptiness
    // of source/view, view read-only flag, and C2 on the held/reopened view.
    fn case(nodes: [ViewNode; 4], full: [bool; 2], readonly: bool, c2: [C2; 2]) -> bool {
        let sudo = ["sudo".to_string()];
        let pick = |nonempty: bool| if nonempty { &sudo[..] } else { &sudo[..0] };
        empty_view(&EmptyView {
            source: [nodes[0], nodes[1]],
            view: [nodes[2], nodes[3]],
            source_entries: pick(full[0]),
            view_entries: pick(full[1]),
            readonly,
            c2,
        })
    }
    let owned = ViewNode {
        dev: 7,
        ino: 42,
        mode: 0o040755,
        uid: 0,
        gid: 0,
    };
    let absent = [C2::Absent; 2];
    assert!(case([owned; 4], [false; 2], true, absent));
    for slot in 0..4 {
        // 0700 would make the low-UID probe see EACCES/unknown, never ENOENT;
        // write bits, special bits, non-directories and foreign owners refuse.
        for mode in [
            0o040700, 0o040711, 0o040750, 0o040775, 0o040757, 0o040777, 0o041755, 0o042755,
            0o100755, 0o120777,
        ] {
            let mut nodes = [owned; 4];
            nodes[slot].mode = mode;
            assert!(
                !case(nodes, [false; 2], true, absent),
                "assertion slot {slot} mode {mode:o}"
            );
        }
        // Foreign owner/group, or any non-fixture inode (even a protected one).
        for change in 0..4 {
            let mut nodes = [owned; 4];
            match change {
                0 => nodes[slot].uid = 1000,
                1 => nodes[slot].gid = 1000,
                2 => nodes[slot].ino = 99,
                _ => nodes[slot].dev = 8,
            }
            assert!(
                !case(nodes, [false; 2], true, absent),
                "assertion slot {slot} change {change}"
            );
        }
    }
    // A writable raw host /usr/local/bin is never admitted as the view.
    let host = ViewNode {
        dev: 1,
        ino: 99,
        mode: 0o040777,
        uid: 0,
        gid: 0,
    };
    assert!(!case([owned, owned, host, host], [false; 2], true, absent));
    assert!(!case([owned; 4], [true, false], true, absent));
    assert!(!case([owned; 4], [false, true], true, absent));
    assert!(!case([owned; 4], [false; 2], false, absent));
    for c2 in [
        [C2::Present, C2::Absent],
        [C2::Absent, C2::Present],
        [C2::Unknown, C2::Absent],
        [C2::Absent, C2::Unknown],
    ] {
        assert!(!case([owned; 4], [false; 2], true, c2));
    }
}

fn recheck_delivery(config: &OwnerConfig, executable: &Path) -> Option<Vec<std::fs::File>> {
    let (prepared, held) = read_fixed_delivery(A7_PROFILE)?;
    let run =
        std::env::var("GITHUB_RUN_ID").ok()? + "-" + &std::env::var("GITHUB_RUN_ATTEMPT").ok()?;
    let mut held = held;
    held.extend(runtime_package()?);
    held.extend(controlled_view(&run, &prepared.accounts)?);
    let pinned = prepared
        .accounts
        .iter()
        .find(|a| a.account == config.account)?;
    (pinned.uid == config.uid
        && pinned.gid == config.gid
        && pinned.workspace == config.workspace
        && pinned.protected_directories == config.protected_directories
        && pinned.environment == config.environment
        && pinned.admin == config.admin
        && pinned.cgroup_parent == config.cgroup_parent
        && pinned.memory_bytes == config.memory_bytes
        && pinned.pids == config.pids
        && (executable == prepared.arxd || executable == prepared.reentry))
        .then_some(held)
}

#[test]
fn a7_a11_delivery_schema_rejections() {
    let account_pin = DeliveryAccount {
        uid: 2000,
        gid: 2001,
        base_dev: 1,
        base_ino: 2,
        workspace_dev: 1,
        workspace_ino: 3,
    };
    assert!(account_record(
        &account_pin,
        2000,
        2001,
        b"/usr/sbin/nologin"
    ));
    assert!(!account_record(
        &account_pin,
        2002,
        2001,
        b"/usr/sbin/nologin"
    ));
    assert!(!account_record(
        &account_pin,
        2000,
        2002,
        b"/usr/sbin/nologin"
    ));
    assert!(!account_record(&account_pin, 2000, 2001, b"/bin/sh"));
    let pin = DeliveryArtifact {
        size: 4,
        sha256: "c".repeat(64),
        dev: 1,
        ino: 2,
    };
    let identity = DeliveryIdentity {
        size: 4,
        dev: 1,
        ino: 2,
        mode: 0o100755,
        uid: 0,
        nlink: 1,
    };
    assert!(artifact_identity(&pin, identity));
    for bad in [
        DeliveryIdentity {
            size: 5,
            ..identity
        },
        DeliveryIdentity { dev: 2, ..identity },
        DeliveryIdentity { ino: 3, ..identity },
        DeliveryIdentity {
            mode: 0o120755,
            ..identity
        },
        DeliveryIdentity {
            mode: 0o100775,
            ..identity
        },
        DeliveryIdentity {
            uid: 1000,
            ..identity
        },
        DeliveryIdentity {
            nlink: 2,
            ..identity
        },
    ] {
        assert!(!artifact_identity(&pin, bad));
    }
    let source = "a".repeat(40);
    let profile = "b".repeat(64);
    let account = |uid| serde_json::json!({"uid":uid,"gid":uid,"base_dev":1,"base_ino":2,"workspace_dev":1,"workspace_ino":3});
    let artifact = serde_json::json!({"size":4,"sha256":"c".repeat(64),"dev":1,"ino":4});
    let good = serde_json::json!({"run":"123-1","source_sha":source,"host_profile_sha256":profile,"accounts":[account(2000),account(2001)],"artifacts":[artifact,artifact]});
    let check = |bytes: &[u8]| {
        delivery_schema(bytes, "123-1", &source, &source, &source, &profile).is_some()
    };
    let bytes = serde_json::to_vec(&good).unwrap();
    assert!(check(&bytes));
    assert!(!check(&[bytes.as_slice(), b"{}"].concat()));
    assert!(!check(&vec![b' '; 4097]));
    for field in [
        "run",
        "source_sha",
        "host_profile_sha256",
        "accounts",
        "artifacts",
    ] {
        let mut bad = good.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(
            !check(&serde_json::to_vec(&bad).unwrap()),
            "missing {field}"
        );
    }
    for (pointer, value) in [
        ("/run", serde_json::json!("124-1")),
        ("/source_sha", serde_json::json!("d".repeat(40))),
        ("/host_profile_sha256", serde_json::json!("d".repeat(64))),
        ("/accounts/1/uid", serde_json::json!(2000)),
        ("/accounts/1/gid", serde_json::json!(2000)),
        ("/accounts/0/uid", serde_json::json!(999)),
        ("/accounts/0/gid", serde_json::json!(true)),
        ("/accounts/0/uid", serde_json::json!(4294967296u64)),
        ("/artifacts/0/size", serde_json::json!(134217729)),
        ("/artifacts/0/sha256", serde_json::json!("invalid")),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(
            !check(&serde_json::to_vec(&bad).unwrap()),
            "assertion {pointer}"
        );
    }
    for pointer in ["", "/accounts/0", "/artifacts/0"] {
        let mut bad = good.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("path".into(), serde_json::json!("/tmp"));
        assert!(!check(&serde_json::to_vec(&bad).unwrap()));
    }
    let text = String::from_utf8(bytes.clone()).unwrap();
    for (key, value) in [("run", "\"123-1\""), ("uid", "2000"), ("size", "4")] {
        let duplicate = text.replacen(
            &format!("\"{key}\":"),
            &format!("\"{key}\":{value},\"{key}\":"),
            1,
        );
        assert!(!check(duplicate.as_bytes()), "duplicate {key}");
    }
    assert!(
        delivery_schema(&bytes, "123-1", &source, &"d".repeat(40), &source, &profile).is_none()
    );
    assert!(
        delivery_schema(&bytes, "123-1", &source, &source, &"d".repeat(40), &profile).is_none()
    );
    let exe = Path::new("/var/lib/arx-ci-123-1/source/target/debug/deps/arxd-abc");
    assert!(delivery_stage(exe, "123-1").is_some());
    for path in [
        "/tmp/arxd-abc",
        "/var/lib/arx-ci-123-1/source/target/debug/deps/arxd-xyz",
        "/var/lib/arx-ci-123-1/source/target/debug/deps/../arxd-abc",
    ] {
        assert!(delivery_stage(Path::new(path), "123-1").is_none());
    }
    assert!(delivery_stage(exe, "124-1").is_none());
}

#[derive(Clone, Copy)]
struct Facts {
    identity: bool,
    exited: bool,
    bounded: bool,
    records: bool,
    rights: bool,
    c1: bool,
    alias_skipped: bool,
    c3_unopened: bool,
    path: bool,
    cloexec: bool,
    regular: bool,
    exec_eof: bool,
    nonzero: bool,
    both_policy_cases: bool,
    cleanup: bool,
}
fn accepts(f: Facts) -> bool {
    f.identity
        && f.exited
        && f.bounded
        && f.records
        && f.rights
        && f.c1
        && f.alias_skipped
        && f.c3_unopened
        && f.path
        && f.cloexec
        && f.regular
        && f.exec_eof
        && f.nonzero
        && f.both_policy_cases
        && f.cleanup
}

fn output(run: &RunResult) -> Option<Vec<u8>> {
    if run.stdout.dropped_bytes != 0
        || run.stderr.dropped_bytes != 0
        || !run.stderr.head.is_empty()
        || !run.stderr.tail.is_empty()
        || run.stdout.head.len() + run.stdout.tail.len() > 4096
    {
        return None;
    }
    Some([run.stdout.head.as_slice(), run.stdout.tail.as_slice()].concat())
}
fn probe_records(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    if !text.ends_with('\n') || bytes.len() > 4096 {
        return false;
    }
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != crate::development::TOOLS.len() + 1 {
        return false;
    }
    for (line, name) in lines.iter().zip(crate::development::TOOLS) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        let Some(object) = value.as_object() else {
            return false;
        };
        if object.len() != 3
            || value["name"] != *name
            || !value["found"].is_boolean()
            || !(value["version"].is_null() || value["version"].is_string())
        {
            return false;
        }
    }
    lines.last() == Some(&r#"{"rights":{"no_new_privs":true,"sudo":"exit_nonzero"}}"#)
}
fn reentry_records(bytes: &[u8]) -> bool {
    if bytes.len() > 4096 {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let prefix = concat!(
        "\nrunning 1 test\ntest unix_shim::rights::linux::tests::e4_sudo_nnp_reentry ... ",
        "ARX_A7_A11_REENTRY_OK\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
    );
    let Some(rest) = text.strip_prefix(prefix) else {
        return false;
    };
    let Some((filtered, elapsed)) = rest.split_once(" filtered out; finished in ") else {
        return false;
    };
    let Some(elapsed) = elapsed.strip_suffix("s\n\n") else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit());
    digits(filtered)
        && elapsed
            .split_once('.')
            .is_some_and(|(a, b)| digits(a) && digits(b))
}
fn valid_run(run: &RunResult, config: &OwnerConfig, residue: bool) -> bool {
    run.identity
        == ObservedIdentity {
            uid: config.uid,
            gid: config.gid,
            daemon_was_root: true,
        }
        && run.outcome == Outcome::Exited
        && run.exit_code == Some(0)
        && run.tree_boundary == TreeBoundary::LinuxCgroupV2
        && run.tree_cleanup_confirmed
        && !residue
}

#[test]
#[ignore = "A7/A11 fixed candidate; D7i review/gates/merge required before native execution"]
fn disposable_linux_a7_a11() {
    let startup = std::time::Instant::now();
    // Refuse BEFORE any host observation, LinuxFs, account or process operation.
    let prepared = prepared_delivery().expect("A7/A11 host/policy/delivery unattested");
    use hierarchy::{linux::LinuxFs, setup::Clock};
    struct Monotonic(std::time::Instant);
    impl Clock for Monotonic {
        fn now(&self) -> Duration {
            self.0.elapsed()
        }
        fn sleep(&self, duration: Duration) {
            std::thread::sleep(duration);
        }
    }
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("ARX_DISPOSABLE_EXEC_TEST", "1"),
    ] {
        assert_eq!(std::env::var(key).as_deref(), Ok(value));
    }
    assert_eq!(unsafe { libc::getuid() }, 0);
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_ne!(prepared.accounts[0].uid, prepared.accounts[1].uid);
    assert_ne!(prepared.accounts[0].gid, prepared.accounts[1].gid);
    assert!(hierarchy::fake::prepare_a1a2(
        &LinuxFs::system(),
        &Monotonic(std::time::Instant::now()),
        &Cancellation::default()
    ));
    let runners = prepared.accounts.each_ref().map(|config| {
        assert!(config.uid >= 1000 && config.gid >= 1000 && !config.admin);
        assert_eq!(config.memory_bytes, 64 * 1024 * 1024);
        assert_eq!(config.pids, 32);
        let _held = recheck_delivery(config, &prepared.arxd).unwrap();
        Runner::new(config.clone()).unwrap()
    });
    assert!(startup.elapsed() <= Duration::from_secs(15));
    crate::service::unix::Notify::send_to(&std::env::var_os("NOTIFY_SOCKET").unwrap(), b"READY=1")
        .unwrap();
    a7_identity_ack();
    let callers = [Cancellation::default(), Cancellation::default()];
    let shutdowns = runners.each_ref().map(Runner::shutdown_handle);
    assert!(a7_cases(
        |index| a7_case(
            &prepared,
            index,
            &runners[index],
            &callers[index],
            &shutdowns[index]
        ),
        || {
            for token in callers.iter().chain(shutdowns.iter()) {
                token.cancel();
            }
        },
        || a7_marker(b"ARX_A7_A11_COMPLETE_OK\n")
    ));
}

// Raw fixed records bypass libtest capture; no identities or host data escape.
fn a7_marker(bytes: &[u8]) -> bool {
    use std::io::Write;
    std::io::stdout().lock().write_all(bytes).is_ok()
}

// Fixture-local ACK envelope; do not widen the shared fixture hold or detach IO.
fn a7_identity_ack() {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        time::Instant,
    };
    let start = Instant::now();
    loop {
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("identity-ack");
        if let Ok(file) = file {
            let m = file.metadata().unwrap();
            assert!(m.is_file() && m.uid() == 0 && m.mode() & 0o7777 == 0o600 && m.nlink() == 1);
            let mut bytes = Vec::new();
            file.take(2).read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"1");
            assert!(start.elapsed() <= Duration::from_secs(5));
            return;
        } else {
            assert_eq!(file.unwrap_err().kind(), std::io::ErrorKind::NotFound);
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn a7_case(
    prepared: &Prepared,
    index: usize,
    runner: &Runner,
    caller: &Cancellation,
    shutdown: &Cancellation,
) -> bool {
    use std::time::Instant;
    let start = Instant::now();
    let config = &prepared.accounts[index];
    let mut calls = [Duration::ZERO; 2];
    for step in 0..2 {
        let spec = RunSpec {
            executable: if step == 0 {
                prepared.arxd.clone()
            } else {
                prepared.reentry.clone()
            },
            arguments: if step == 0 {
                vec!["tool-probe".into()]
            } else {
                vec![
                    "--exact".into(),
                    "unix_shim::rights::linux::tests::e4_sudo_nnp_reentry".into(),
                    "--ignored".into(),
                    "--nocapture".into(),
                    "--test-threads=1".into(),
                    "--color=never".into(),
                ]
            },
            cwd: config.workspace.clone(),
            timeout: Duration::from_secs(if step == 0 { 55 } else { 10 }),
            output_bytes: 4096,
        };
        // Reopen NOFOLLOW+CLOEXEC and retain all account/view/artifact pins.
        let Some(_held) = recheck_delivery(config, &spec.executable) else {
            return false;
        };
        if start.elapsed().saturating_sub(calls.iter().sum()) > Duration::from_secs(15) {
            return false;
        }
        let call = Instant::now();
        let result = runner.run_phased(&spec, caller, shutdown);
        // Entire synchronous call, including error returns and native guard drops.
        calls[step] = call.elapsed();
        if calls[step] > Duration::from_secs(if step == 0 { 60 } else { 15 }) {
            return false;
        }
        let Ok(result) = result else {
            return false;
        };
        if !valid_run(&result, config, runner.cleanup_residue()) {
            return false;
        }
        let Some(bytes) = output(&result) else {
            return false;
        };
        if !(if step == 0 {
            probe_records(&bytes)
        } else {
            reentry_records(&bytes)
        }) {
            return false;
        }
    }
    let marked = a7_marker(if index == 0 {
        b"ARX_A7_A11_CASE0_OK\n"
    } else {
        b"ARX_A7_A11_CASE1_OK\n"
    });
    let total = start.elapsed();
    marked && a7_envelopes(calls, total.saturating_sub(calls.iter().sum()), total)
}

// Fixed two-case scope, shared by pure counterfactuals and the ignored controller.
// Catch inside each worker so failure cancellation precedes any implicit join.
fn a7_cases(
    work: impl Fn(usize) -> bool + Sync,
    cancel: impl Fn() + Sync,
    complete: impl FnOnce() -> bool,
) -> bool {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        time::Instant,
    };
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        let cases_start = Instant::now();
        let handles = [0, 1].map(|index| {
            let tx = tx.clone();
            let work = &work;
            let cancel = &cancel;
            scope.spawn(move || {
                let good = catch_unwind(AssertUnwindSafe(|| work(index))).unwrap_or(false);
                if !good {
                    cancel();
                }
                let _ = tx.send(good);
                good
            })
        });
        drop(tx);
        let mut good = true;
        // Receive either completion, never join a healthy worker while hiding failure.
        for _ in 0..2 {
            let remaining = Duration::from_secs(90).saturating_sub(cases_start.elapsed());
            if rx.recv_timeout(remaining) != Ok(true) {
                good = false;
                cancel();
                break;
            }
        }
        let join = Instant::now();
        // Do not short-circuit: BOTH joins run even if either reports a panic.
        let joined = handles.map(|handle| handle.join().unwrap_or(false));
        let accepted = a7_completion(good, joined, join.elapsed());
        accepted && complete() && join.elapsed() <= Duration::from_secs(10)
    })
}
fn a7_completion(good: bool, joined: [bool; 2], elapsed: Duration) -> bool {
    good && joined == [true, true] && elapsed <= Duration::from_secs(10)
}
fn a7_envelopes(calls: [Duration; 2], checks: Duration, total: Duration) -> bool {
    calls[0] <= Duration::from_secs(60)
        && calls[1] <= Duration::from_secs(15)
        && checks <= Duration::from_secs(15)
        && total <= Duration::from_secs(90)
}

#[test]
fn a7_a11_scheduling_counterfactuals() {
    use std::sync::{
        Barrier,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let d = Duration::from_secs;
    assert!(a7_completion(true, [true, true], d(10)));
    for (good, joined, elapsed) in [
        (false, [true, true], d(0)),
        (true, [false, true], d(0)),
        (true, [true, false], d(0)),
        (true, [true, true], d(11)),
    ] {
        assert!(!a7_completion(good, joined, elapsed));
    }
    assert!(a7_envelopes([d(60), d(15)], d(15), d(90)));
    for (calls, checks, total) in [
        ([d(61), d(0)], d(0), d(61)),
        ([d(0), d(16)], d(0), d(16)),
        ([d(1), d(1)], d(16), d(18)),
        ([d(60), d(15)], d(15), d(91)),
    ] {
        assert!(!a7_envelopes(calls, checks, total));
    }
    for failed in 0..2 {
        for panic in [false, true] {
            let cancelled = AtomicBool::new(false);
            let finished = AtomicUsize::new(0);
            let barrier = Barrier::new(2);
            assert!(!a7_cases(
                |index| {
                    barrier.wait();
                    if index == failed {
                        finished.fetch_add(1, Ordering::SeqCst);
                        assert!(!panic, "injected worker panic");
                        return false;
                    }
                    let wait = std::time::Instant::now();
                    while !cancelled.load(Ordering::SeqCst)
                        && wait.elapsed() < Duration::from_secs(1)
                    {
                        std::thread::yield_now();
                    }
                    finished.fetch_add(1, Ordering::SeqCst);
                    true
                },
                || cancelled.store(true, Ordering::SeqCst),
                || panic!("completion after failed worker")
            ));
            assert!(cancelled.load(Ordering::SeqCst));
            assert_eq!(finished.load(Ordering::SeqCst), 2);
        }
    }
    let completed = AtomicUsize::new(0);
    assert!(a7_cases(
        |_| {
            completed.fetch_add(1, Ordering::SeqCst);
            true
        },
        || {},
        || true
    ));
    assert_eq!(completed.load(Ordering::SeqCst), 2);
}

// Pure discriminators, also compiled in isolation by the source-only check.
#[test]
fn a7_a11_discriminators() {
    let good = Facts {
        identity: true,
        exited: true,
        bounded: true,
        records: true,
        rights: true,
        c1: true,
        alias_skipped: true,
        c3_unopened: true,
        path: true,
        cloexec: true,
        regular: true,
        exec_eof: true,
        nonzero: true,
        both_policy_cases: true,
        cleanup: true,
    };
    assert!(accepts(good));
    for index in 0..15 {
        let mut bad = good;
        match index {
            0 => bad.identity = false,
            1 => bad.exited = false,
            2 => bad.bounded = false,
            3 => bad.records = false,
            4 => bad.rights = false,
            5 => bad.c1 = false,
            6 => bad.alias_skipped = false,
            7 => bad.c3_unopened = false,
            8 => bad.path = false,
            9 => bad.cloexec = false,
            10 => bad.regular = false,
            11 => bad.exec_eof = false,
            12 => bad.nonzero = false,
            13 => bad.both_policy_cases = false,
            _ => bad.cleanup = false,
        }
        assert!(!accepts(bad), "assertion discriminator {index}");
    }
}
#[test]
fn a7_a11_delivery_stays_closed() {
    assert!(prepared_delivery().is_none());
}

#[test]
fn a7_a11_record_discriminators() {
    let mut probe = Vec::new();
    for name in crate::development::TOOLS {
        probe.extend_from_slice(
            format!("{{\"name\":\"{name}\",\"found\":false,\"version\":null}}\n").as_bytes(),
        );
    }
    let rights = b"{\"rights\":{\"no_new_privs\":true,\"sudo\":\"exit_nonzero\"}}\n";
    probe.extend_from_slice(rights);
    assert!(probe_records(&probe));
    for result in ["exit_zero", "unknown", "absent", "untrusted"] {
        let bad = String::from_utf8(probe.clone())
            .unwrap()
            .replace("exit_nonzero", result);
        assert!(!probe_records(bad.as_bytes()));
    }
    assert!(!probe_records(&probe[..probe.len() - 1]));
    assert!(!probe_records(&[probe.as_slice(), b"extra\n"].concat()));
    let tool = probe.split_inclusive(|b| *b == b'\n').next().unwrap();
    assert!(!probe_records(
        &[&probe[..probe.len() - rights.len()], tool, rights].concat()
    ));
    assert!(!probe_records(&vec![b'x'; 4097]));
    assert!(!probe_records(rights));
    let reentry = concat!(
        "\nrunning 1 test\ntest unix_shim::rights::linux::tests::e4_sudo_nnp_reentry ... ",
        "ARX_A7_A11_REENTRY_OK\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ",
        "500 filtered out; finished in 0.01s\n\n"
    );
    assert!(reentry_records(reentry.as_bytes()));
    assert!(!reentry_records(&[reentry.as_bytes(), b"extra\n"].concat()));
    assert!(!reentry_records(
        reentry.replace("0 failed", "1 failed").as_bytes()
    ));
    assert!(!reentry_records(reentry.trim_end().as_bytes()));
}

#[test]
fn a7_a11_run_discriminators() {
    let config = OwnerConfig {
        account: "fixture".into(),
        uid: 2000,
        gid: 2001,
        workspace: "/fixture".into(),
        protected_directories: vec![],
        environment: BTreeMap::new(),
        admin: false,
        cgroup_parent: None,
        memory_bytes: 67108864,
        pids: 32,
    };
    let mut run = RunResult {
        outcome: Outcome::Exited,
        exit_code: Some(0),
        identity: ObservedIdentity {
            uid: 2000,
            gid: 2001,
            daemon_was_root: true,
        },
        tree_boundary: TreeBoundary::LinuxCgroupV2,
        tree_cleanup_confirmed: true,
        stdout: CapturedOutput {
            head: b"ok".to_vec(),
            tail: vec![],
            dropped_bytes: 0,
        },
        stderr: CapturedOutput {
            head: vec![],
            tail: vec![],
            dropped_bytes: 0,
        },
    };
    assert!(valid_run(&run, &config, false));
    assert!(!valid_run(&run, &config, true));
    run.identity.uid += 1;
    assert!(!valid_run(&run, &config, false));
    run.identity.uid -= 1;
    run.identity.gid += 1;
    assert!(!valid_run(&run, &config, false));
    run.identity.gid -= 1;
    run.identity.daemon_was_root = false;
    assert!(!valid_run(&run, &config, false));
    run.identity.daemon_was_root = true;
    run.exit_code = Some(1);
    assert!(!valid_run(&run, &config, false));
    run.exit_code = None;
    assert!(!valid_run(&run, &config, false));
    run.exit_code = Some(0);
    run.outcome = Outcome::TimedOut;
    assert!(!valid_run(&run, &config, false));
    run.outcome = Outcome::Exited;
    run.tree_cleanup_confirmed = false;
    assert!(!valid_run(&run, &config, false));
    run.tree_cleanup_confirmed = true;
    run.tree_boundary = TreeBoundary::MacProcessGroupBestEffort;
    assert!(!valid_run(&run, &config, false));
    assert!(output(&run).is_some());
    run.stdout.dropped_bytes = 1;
    assert!(output(&run).is_none());
    run.stdout.dropped_bytes = 0;
    run.stderr.dropped_bytes = 1;
    assert!(output(&run).is_none());
    run.stderr.dropped_bytes = 0;
    run.stderr.head.push(1);
    assert!(output(&run).is_none());
    run.stderr.head.clear();
    run.stderr.tail.push(1);
    assert!(output(&run).is_none());
    run.stderr.tail.clear();
    run.stdout.head = vec![0; 4097];
    assert!(output(&run).is_none());
}
