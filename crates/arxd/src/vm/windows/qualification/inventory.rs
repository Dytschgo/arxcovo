//! Exact reviewed bundle inventory. Existing entries are pinned by relative
//! opens with read-only sharing. Directory ACLs supply namespace protection
//! against new entries for the paused trial.

use super::*;
use crate::vm::public_egress::{REVIEWED_EGRESS_BUILDS, ReviewedEgressBuild};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Seek, SeekFrom},
};
use windows_sys::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};

const PAYLOAD_ENTRIES: usize = crate::vm::qemu_distribution::STOCK.payload_files;
const MAX_MANIFEST_BYTES: usize = 32_768;
const MAX_PAYLOAD_FILE: u64 = 512 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_DIRECTORIES: usize = 64;
const INVENTORY_TIME: Duration = Duration::from_secs(10);
const REFUSAL: NativeRefusal = NativeRefusal::DistributionUnverified;

pub(in crate::vm::windows) struct Bundle {
    pub(in crate::vm::windows) root: PathPin,
    directories: BTreeMap<String, File>,
    files: BTreeMap<String, (File, u64)>,
}

impl Bundle {
    pub(in crate::vm::windows) fn check(&self, deadline: Instant) -> Result<(), NativeRefusal> {
        let entries = std::iter::once(("", &self.root.leaf)).chain(
            self.directories
                .iter()
                .map(|(name, dir)| (name.as_str(), dir)),
        );
        let mut files = self
            .files
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut dirs = self
            .directories
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for (parent, dir) in entries {
            timely(deadline)?;
            file_identity(dir, true).map_err(|_| REFUSAL)?;
            for (name, attributes) in listing_bounded(dir, files.len() + dirs.len(), Some(deadline))
                .map_err(|_| REFUSAL)?
            {
                timely(deadline)?;
                let path = if parent.is_empty() {
                    name
                } else {
                    format!("{parent}/{name}")
                };
                if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(REFUSAL);
                }
                if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                    if !dirs.remove(path.as_str()) {
                        return Err(REFUSAL);
                    }
                } else {
                    if !files.remove(path.as_str()) {
                        return Err(REFUSAL);
                    }
                    let (file, size) = &self.files[&path];
                    file_identity(file, false).map_err(|_| REFUSAL)?;
                    if file.metadata().map_err(|_| REFUSAL)?.len() != *size {
                        return Err(REFUSAL);
                    }
                }
            }
        }
        if !files.is_empty() || !dirs.is_empty() {
            return Err(REFUSAL);
        }
        timely(deadline)
    }

    pub(in crate::vm::windows) fn identity(
        &self,
        name: &str,
    ) -> Result<FileIdentity, NativeRefusal> {
        let (file, _) = self.files.get(name).ok_or(REFUSAL)?;
        Ok(file_identity(file, false).map_err(|_| REFUSAL)?.0)
    }

    pub(super) fn directory_identity(&self, name: &str) -> Result<FileIdentity, NativeRefusal> {
        let dir = self.directories.get(name).ok_or(REFUSAL)?;
        Ok(file_identity(dir, true).map_err(|_| REFUSAL)?.0)
    }

    pub(super) fn protected(&self, owner: PSID, deadline: Instant) -> Result<(), NativeRefusal> {
        if !std::iter::once(&self.root.leaf)
            .chain(self.directories.values())
            .all(|dir| Instant::now() < deadline && protected_directory(dir, owner))
        {
            return Err(REFUSAL);
        }
        self.check(deadline)?;
        binding(&self.root)?;
        timely(deadline)
    }
}

pub(in crate::vm::windows) fn safe_dll_search() -> bool {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    let mut value = 0u32;
    let mut size = size_of_val(&value) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SYSTEM\CurrentControlSet\Control\Session Manager").as_ptr(),
            wide("SafeDllSearchMode").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            ptr_of(&mut value),
            &mut size,
        )
    };
    // Safe DLL search is on by default. An explicit value must enable it.
    status == ERROR_FILE_NOT_FOUND || (status == ERROR_SUCCESS && size == 4 && value == 1)
}

pub(in crate::vm::windows) fn restricted_environment(
    cwd: &LocalPath,
) -> Result<Vec<u16>, NativeRefusal> {
    let mut root = [0u16; 260];
    let size = unsafe { GetSystemWindowsDirectoryW(root.as_mut_ptr(), root.len() as u32) } as usize;
    let reason = NativeRefusal::PathCustodyUnverified;
    if size == 0 || size >= root.len() {
        return Err(reason);
    }
    let root = String::from_utf16(&root[..size]).map_err(|_| reason)?;
    let mut env = Vec::new();
    for (key, value) in [
        ("SystemRoot", root.as_str()),
        ("TEMP", cwd.as_str()),
        ("TMP", cwd.as_str()),
        ("WINDIR", root.as_str()),
    ] {
        env.extend(format!("{key}={value}").encode_utf16().chain(Some(0)));
    }
    env.push(0);
    Ok(env)
}

/// The shared initial search namespace. The working directory remains
/// held and empty. The bundle retains every reviewed exact-case leaf.
pub(in crate::vm::windows) fn validate_launch_namespace(
    bundle: &Bundle,
    cwd: &PathPin,
    owner: PSID,
    safe_search: bool,
) -> Result<(), NativeRefusal> {
    let deadline = Instant::now() + Duration::from_secs(2);
    if !safe_search
        || !protected_directory(&cwd.leaf, owner)
        || !listing_bounded(&cwd.leaf, 0, Some(deadline)).is_ok_and(|entries| entries.is_empty())
    {
        return Err(NativeRefusal::PathCustodyUnverified);
    }
    binding(cwd)?;
    bundle.protected(owner, deadline)
}

pub(in crate::vm::windows) fn binding(pin: &PathPin) -> Result<(), NativeRefusal> {
    // PathPin retains every non-reparse component. Reconfirm the one
    // external lookup, the drive letter, against the held volume root.
    let root = pin._ancestors.first().unwrap_or(&pin.leaf);
    let identity = file_identity(root, true).map_err(|_| REFUSAL)?.0;
    verify_volume_root(&pin.path.as_str()[..3], identity).map_err(|_| REFUSAL)
}

pub(super) fn protected_directory(dir: &File, owner: PSID) -> bool {
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    let (mut actual_owner, mut dacl, mut descriptor) = (null_mut(), null_mut(), null_mut());
    // The descriptor backs the returned SID and DACL until LocalFree.
    let status = unsafe {
        GetSecurityInfo(
            dir.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut actual_owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }
    // GetSecurityInfo owns both values until LocalFree.
    let protected = unsafe { protected_descriptor(actual_owner, dacl, owner) };
    unsafe { LocalFree(descriptor) };
    protected
}

/// All non-null pointers are backed by a live Windows security descriptor
/// or by the enrolled account SID.
unsafe fn protected_descriptor(actual_owner: PSID, dacl: *const ACL, owner: PSID) -> bool {
    // Untrusted principals cannot change the DACL, delete children or add
    // entries. The verified CurrentUser token and its child have no enabled
    // administrators group. Same-account tampering is outside this boundary.
    let trusted_writer = |sid| unsafe {
        IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
            || IsWellKnownSid(sid, WinLocalSystemSid) != 0
    };
    unsafe {
        !dacl.is_null()
            && !actual_owner.is_null()
            && (EqualSid(actual_owner, owner) != 0 || trusted_writer(actual_owner))
            && (*dacl).AceCount <= 64
            && (0..u32::from((*dacl).AceCount)).all(|index| {
                let mut ace = null_mut();
                if GetAce(dacl, index, &mut ace) == 0 {
                    return false;
                }
                let header = &*ace.cast::<ACE_HEADER>();
                match u32::from(header.AceType) {
                    ACCESS_DENIED_ACE_TYPE => true,
                    ACCESS_ALLOWED_ACE_TYPE => {
                        let allow = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                        let sid = std::ptr::addr_of!(allow.SidStart).cast_mut().cast();
                        let readonly = FILE_GENERIC_READ
                            | FILE_GENERIC_EXECUTE
                            | GENERIC_READ
                            | GENERIC_EXECUTE;
                        allow.Mask & !readonly == 0 || trusted_writer(sid)
                    }
                    _ => false,
                }
            })
    }
}

impl Bundle {
    fn add_file(&mut self, name: &str) -> Result<&File, NativeRefusal> {
        let parent = if let Some((path, _)) = name.rsplit_once('/') {
            let mut parent = &self.root.leaf;
            let mut key = String::new();
            for part in path.split('/') {
                if !key.is_empty() {
                    key.push('/');
                }
                key.push_str(part);
                if !self.directories.contains_key(&key) {
                    if self.directories.len() >= MAX_DIRECTORIES {
                        return Err(REFUSAL);
                    }
                    let dir = open_relative(parent, part, Node::Directory, false, false)
                        .map_err(|_| REFUSAL)?;
                    file_identity(&dir, true).map_err(|_| REFUSAL)?;
                    self.directories.insert(key.clone(), dir);
                }
                parent = &self.directories[&key];
            }
            parent
        } else {
            &self.root.leaf
        };
        let name_leaf = name.rsplit('/').next().ok_or(REFUSAL)?;
        let file =
            open_relative(parent, name_leaf, Node::File, false, false).map_err(|_| REFUSAL)?;
        file_identity(&file, false).map_err(|_| REFUSAL)?;
        let size = file.metadata().map_err(|_| REFUSAL)?.len();
        if self.files.insert(name.to_owned(), (file, size)).is_some() {
            return Err(REFUSAL);
        }
        Ok(&self.files[name].0)
    }

    fn open(root: &LocalPath) -> Result<Self, NativeRefusal> {
        Ok(Self {
            root: PathPin::open(root, true, false).map_err(|_| REFUSAL)?,
            directories: BTreeMap::new(),
            files: BTreeMap::new(),
        })
    }

    fn payload(
        &mut self,
        manifest: &[u8],
        count: usize,
        deadline: Instant,
        roles: &[(&str, ArtifactPin)],
    ) -> Result<u64, NativeRefusal> {
        let entries = parse_manifest(manifest, count).ok_or(REFUSAL)?;
        // A role must exist under its exact spelling even if all other
        // payload entries have valid hashes.
        if roles.iter().any(|&(role, pin)| {
            !entries
                .iter()
                .any(|&(name, hash)| name == role && hash == pin.sha256)
        }) {
            return Err(REFUSAL);
        }
        let mut total = 0u64;
        for (name, expected) in entries {
            timely(deadline)?;
            let mut file = self.add_file(name)?;
            let size = file.metadata().map_err(|_| REFUSAL)?.len();
            total = total
                .checked_add(size)
                .filter(|total| *total <= MAX_PAYLOAD_BYTES && size <= MAX_PAYLOAD_FILE)
                .ok_or(REFUSAL)?;
            if roles
                .iter()
                .any(|&(role, pin)| name == role && size != pin.size)
            {
                return Err(REFUSAL);
            }
            file.seek(SeekFrom::Start(0)).map_err(|_| REFUSAL)?;
            let input = BoundedFile {
                inner: file,
                deadline,
            };
            if !matches!(
                ArtifactPin {
                    size,
                    sha256: expected
                }
                .verify(input, MAX_PAYLOAD_FILE),
                Ok(true)
            ) {
                return Err(REFUSAL);
            }
        }
        self.check(deadline)?;
        Ok(total)
    }
}

fn timely(deadline: Instant) -> Result<(), NativeRefusal> {
    (Instant::now() < deadline).then_some(()).ok_or(REFUSAL)
}

struct BoundedFile<'a> {
    inner: &'a File,
    deadline: Instant,
}

impl Read for BoundedFile<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.inner.read(buffer)
    }
}

pub(in crate::vm::windows) fn collect(
    root: &LocalPath,
) -> Result<(Distribution, Bundle), NativeRefusal> {
    use crate::vm::qemu_distribution::STOCK;

    collect_exact(
        root,
        STOCK.manifest,
        STOCK.receipt,
        STOCK.payload_files,
        &[
            ("qemu-system-x86_64.exe", STOCK.system),
            ("qemu-img.exe", STOCK.image),
        ],
    )
}

/// A separate read-only inventory lane. Stock trial callers continue to use
/// `collect`; the patched proof grants no launch or TEST authority.
pub(super) struct PatchedBundle {
    pub(super) bundle: Bundle,
    pub(super) review: ReviewedEgressBuild,
}

impl PatchedBundle {
    pub(super) fn hold(
        root: &LocalPath,
        binary: crate::vm::public_egress::QualifiedEgressBinary,
        owner: PSID,
    ) -> Result<Self, NativeRefusal> {
        let pins = binary.pin();
        let review = *REVIEWED_EGRESS_BUILDS
            .iter()
            .find(|build| build.qemu == pins.qemu && build.receipt == pins.receipt)
            .ok_or(REFUSAL)?;
        let (_, bundle) = collect_exact(
            root,
            review.manifest,
            review.receipt,
            review.payload_count,
            &review.execution_roles(),
        )?;
        bundle.protected(owner, Instant::now() + INVENTORY_TIME)?;
        Ok(Self { bundle, review })
    }
}

fn collect_exact(
    root: &LocalPath,
    manifest_pin: ArtifactPin,
    receipt_pin: ArtifactPin,
    count: usize,
    roles: &[(&str, ArtifactPin)],
) -> Result<(Distribution, Bundle), NativeRefusal> {
    let deadline = Instant::now() + INVENTORY_TIME;
    let mut bundle = Bundle::open(root)?;
    let receipt =
        read_bounded(bundle.add_file("RECEIPT.txt")?, receipt_pin.size).map_err(|_| REFUSAL)?;
    let manifest = read_bounded(bundle.add_file("MANIFEST.sha256")?, manifest_pin.size)
        .map_err(|_| REFUSAL)?;
    let receipt_hash: [u8; 32] = Sha256::digest(&receipt).into();
    let manifest_hash: [u8; 32] = Sha256::digest(&manifest).into();
    if receipt.len() as u64 != receipt_pin.size
        || receipt_hash != receipt_pin.sha256
        || manifest.len() as u64 != manifest_pin.size
        || manifest_hash != manifest_pin.sha256
    {
        return Err(REFUSAL);
    }

    let payload_bytes = bundle.payload(&manifest, count, deadline, roles)?;
    Ok((
        Distribution {
            receipt: receipt_hash,
            receipt_bytes: receipt_pin.size,
            manifest: manifest_hash,
            manifest_bytes: manifest_pin.size,
            payload_files: count as u16,
            payload_bytes,
        },
        bundle,
    ))
}

fn valid_manifest_name(name: &str) -> bool {
    // Treat commas like underscores for spelling validation only. The original
    // name is used for uniqueness, held relative opens and inventory checks.
    !name.is_empty()
        && name.len() <= 180
        && !name.contains(['\\', ':'])
        && LocalPath::parse(&format!("D:\\{}", name.replace(',', "_"))).is_ok()
}

fn parse_manifest(bytes: &[u8], count: usize) -> Option<Vec<(&str, [u8; 32])>> {
    if bytes.len() > MAX_MANIFEST_BYTES || count > PAYLOAD_ENTRIES {
        return None;
    }
    let raw = std::str::from_utf8(bytes).ok()?;
    let mut entries = Vec::with_capacity(count);
    let mut seen = BTreeSet::from(["manifest.sha256".to_owned(), "receipt.txt".to_owned()]);
    for line in raw.lines() {
        let (digest, name) = line.split_once("  ")?;
        if entries.len() == count
            || !valid_manifest_name(name)
            || !seen.insert(name.to_ascii_lowercase())
        {
            return None;
        }
        entries.push((name, sha256_spelling(digest)?));
    }
    (entries.len() == count).then_some(entries)
}

fn sha256_spelling(digest: &str) -> Option<[u8; 32]> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut bytes = [0; 32];
    hex::decode_to_slice(digest, &mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::fs;

    pub(super) const NAME: &str = "share/Firmware,ROM.bin";
    const BYTES: &[u8] = b"reviewed firmware fixture";

    pub(in crate::vm::windows::qualification) struct Fixture {
        pub(in crate::vm::windows::qualification) root: tempfile::TempDir,
        path: LocalPath,
        manifest: String,
        count: usize,
    }

    impl Fixture {
        pub(in crate::vm::windows::qualification) fn new() -> Self {
            Self::with_payload(NAME, BYTES)
        }

        pub(in crate::vm::windows::qualification) fn with_payload(
            name: &str,
            bytes: &[u8],
        ) -> Self {
            Self::with_payloads(&[(name, bytes)])
        }

        pub(in crate::vm::windows::qualification) fn with_payloads(
            files: &[(&str, &[u8])],
        ) -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = local(root.path().to_str().unwrap()).unwrap();
            let mut manifest = String::new();
            for (name, bytes) in files {
                use std::fmt::Write;
                let file = root.path().join(name);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, bytes).unwrap();
                writeln!(manifest, "{}  {name}", hex::encode(Sha256::digest(bytes))).unwrap();
            }
            fs::write(root.path().join("RECEIPT.txt"), b"receipt").unwrap();
            fs::write(root.path().join("MANIFEST.sha256"), &manifest).unwrap();
            Self {
                root,
                path,
                manifest,
                count: files.len(),
            }
        }

        pub(in crate::vm::windows::qualification) fn inspect(
            &self,
            deadline: Instant,
        ) -> Result<Bundle, NativeRefusal> {
            let mut bundle = Bundle::open(&self.path)?;
            bundle.add_file("RECEIPT.txt")?;
            bundle.add_file("MANIFEST.sha256")?;
            bundle.payload(self.manifest.as_bytes(), self.count, deadline, &[])?;
            Ok(bundle)
        }

        pub(in crate::vm::windows::qualification) fn collect_with_receipt(
            &self,
            receipt: ArtifactPin,
        ) -> Result<(Distribution, Bundle), NativeRefusal> {
            let manifest = ArtifactPin {
                size: self.manifest.len() as u64,
                sha256: Sha256::digest(self.manifest.as_bytes()).into(),
            };
            collect_exact(&self.path, manifest, receipt, self.count, &[])
        }

        fn refuse(&self) {
            assert_eq!(
                self.inspect(Instant::now() + INVENTORY_TIME).err(),
                Some(REFUSAL)
            );
        }
    }

    #[test]
    fn manifest_is_bounded_and_refuses_aliases_and_duplicate_names() {
        let digest = "a".repeat(64);
        let valid = format!("{digest}  tools/a,b.exe\r\n{digest}  z.dll\r\n");
        assert_eq!(
            parse_manifest(valid.as_bytes(), 2).unwrap()[0].0,
            "tools/a,b.exe"
        );
        for name in [
            "",
            "tools/../a,b.exe",
            "tools/./a,b.exe",
            "tools//a,b.exe",
            "tools/a,b.exe/",
            "C:/a,b.exe",
            "/a,b.exe",
            r"a\z,b.dll",
            "tools/A,B.exe",
            "tools/NUL.a,b",
            "COM1/tools/a,b.exe",
            "tools/a,b.exe.",
            "tools/a,b.exe ",
            "receipt.txt",
            "z,b.dll:ads",
        ] {
            let bad = format!("{digest}  tools/a,b.exe\n{digest}  {name}\n");
            assert!(parse_manifest(bad.as_bytes(), 2).is_none(), "{name:?}");
        }
        let too_long = format!("{digest}  {},\n", "a".repeat(180));
        assert!(parse_manifest(too_long.as_bytes(), 1).is_none());
        assert!(parse_manifest(valid.as_bytes(), 1).is_none());
        assert!(parse_manifest(&[b'a'; MAX_MANIFEST_BYTES + 1], 2).is_none());
    }

    pub(in crate::vm::windows::qualification) const READ_ONLY: &str =
        "D:P(A;OICI;GRGX;;;WD)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
    pub(in crate::vm::windows::qualification) const WRITABLE: &str = "D:P(A;OICI;GA;;;WD)";

    struct Descriptor(PSECURITY_DESCRIPTOR);

    impl Descriptor {
        fn from_sddl(sddl: &str) -> Self {
            use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
            let mut descriptor = null_mut();
            succeeded(unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(sddl).as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            })
            .unwrap();
            Self(descriptor)
        }

        fn dacl(&self) -> *mut ACL {
            let (mut present, mut defaulted, mut dacl) = (0, 0, null_mut());
            succeeded(unsafe {
                GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted)
            })
            .unwrap();
            dacl
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0) };
        }
    }

    pub(in crate::vm::windows::qualification) fn set_dacl(path: &Path, sddl: &str) {
        use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
        let descriptor = Descriptor::from_sddl(sddl);
        assert_eq!(
            unsafe {
                SetNamedSecurityInfoW(
                    wide(path).as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    descriptor.dacl(),
                    null_mut(),
                )
            },
            ERROR_SUCCESS
        );
    }

    #[test]
    fn directory_policy_requires_an_enrolled_or_system_owner() {
        let owner = canonical_sid("S-1-5-21-1-2-3-1001").unwrap();
        let foreign = canonical_sid("S-1-5-21-1-2-3-1002").unwrap();
        let system = canonical_sid("S-1-5-18").unwrap();
        let acl = Descriptor::from_sddl(READ_ONLY);
        assert!(unsafe { protected_descriptor(owner.0, acl.dacl(), owner.0) });
        assert!(unsafe { protected_descriptor(system.0, acl.dacl(), owner.0) });
        assert!(!unsafe { protected_descriptor(foreign.0, acl.dacl(), owner.0) });
    }

    #[test]
    fn directory_policy_refuses_unknown_ace_types() {
        let owner = canonical_sid("S-1-5-21-1-2-3-1001").unwrap();
        let acl = Descriptor::from_sddl(
            "D:P(OD;;GW;a5140930-5512-42d8-aa70-7d472a726228;;WD)(A;;GRGX;;;WD)",
        );
        assert!(!unsafe { protected_descriptor(owner.0, acl.dacl(), owner.0) });
    }

    #[test]
    fn directory_policy_refuses_own_sid_writes_inherit_only_writes_and_null_dacl() {
        let raw = "S-1-5-21-1-2-3-1001";
        let owner = canonical_sid(raw).unwrap();
        for sddl in [
            format!("D:P(A;;GRGX;;;WD)(A;;GW;;;{raw})"),
            "D:P(A;;GRGX;;;WD)(A;OICIIO;GA;;;CO)".to_owned(),
            "D:P(D;;GW;;;WD)(A;;GRGW;;;WD)".to_owned(),
            "D:NO_ACCESS_CONTROL".to_owned(),
        ] {
            let acl = Descriptor::from_sddl(&sddl);
            assert!(
                !unsafe { protected_descriptor(owner.0, acl.dacl(), owner.0) },
                "{sddl}"
            );
        }
    }

    #[test]
    fn distribution_opens_and_lists_only_through_held_directories() {
        let held = Fixture::new();
        let sibling = Fixture::new();
        fs::remove_file(held.root.path().join(NAME)).unwrap();
        fs::write(sibling.root.path().join("share/sibling.bin"), BYTES).unwrap();
        assert!(sibling.root.path().join(NAME).is_file());
        held.refuse();

        fs::write(held.root.path().join(NAME), BYTES).unwrap();
        let mut bundle = held.inspect(Instant::now() + INVENTORY_TIME).unwrap();
        // The advisory spelling now points at a distinct directory with
        // an extra entry. Only the held tree may answer the inventory.
        bundle.root.path = sibling.path.clone();
        assert!(bundle.check(Instant::now() + INVENTORY_TIME).is_ok());
        fs::write(held.root.path().join("share/held-only.bin"), BYTES).unwrap();
        assert!(!sibling.root.path().join("share/held-only.bin").exists());
        assert_eq!(bundle.check(Instant::now() + INVENTORY_TIME), Err(REFUSAL));
    }

    #[test]
    fn directory_acl_excludes_writers_that_can_add_unlisted_entries() {
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;

        let user = current_user().unwrap();
        let root = tempfile::tempdir().unwrap();
        let cases = [
            (
                "read-only",
                "D:P(A;;GRGX;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)",
                true,
            ),
            ("add-file", "D:P(A;;GRGXGW;;;WD)", false),
            ("add-dir", "D:P(A;;0x4;;;WD)(A;;GRGX;;;WD)", false),
        ];
        for (name, policy, protected) in cases {
            let mut descriptor = null_mut();
            succeeded(unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(policy).as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            })
            .unwrap();
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let path = root.path().join(name);
            let created = unsafe { CreateDirectoryW(wide(&path).as_ptr(), &attributes) };
            unsafe { LocalFree(descriptor) };
            succeeded(created).unwrap();
            let held = open_directory(path.to_str().unwrap()).unwrap();
            assert_eq!(
                protected_directory(&held, user_sid(&user)),
                protected,
                "{name}"
            );
        }
    }

    #[test]
    fn inventory_accepts_exact_names_and_refuses_an_unpinned_receipt() {
        let f = Fixture::new();
        assert!(f.inspect(Instant::now() + INVENTORY_TIME).is_ok());
        assert_eq!(collect(&f.path).err(), Some(REFUSAL));
    }

    #[test]
    fn inventory_refuses_extra_root_and_nested_entries() {
        for extra in ["planted.dll", "share/preexisting.txt", "share/empty"] {
            let f = Fixture::new();
            let path = f.root.path().join(extra);
            if extra.ends_with("empty") {
                fs::create_dir(path).unwrap();
            } else {
                fs::write(path, b"unlisted").unwrap();
            }
            f.refuse();
        }
    }

    #[test]
    fn inventory_refuses_missing_renamed_and_case_variant_files() {
        for replacement in [
            None,
            Some("share/another.bin"),
            Some("share/firmware,rom.bin"),
        ] {
            let f = Fixture::new();
            let original = f.root.path().join(NAME);
            if let Some(replacement) = replacement {
                let intermediate = f.root.path().join("share/temp.bin");
                fs::rename(original, &intermediate).unwrap();
                fs::rename(intermediate, f.root.path().join(replacement)).unwrap();
            } else {
                fs::remove_file(original).unwrap();
            }
            f.refuse();
        }
    }

    #[test]
    fn inventory_refuses_wrong_size_and_same_size_wrong_digest() {
        for impostor in [vec![b'x'; BYTES.len() + 1], vec![b'x'; BYTES.len()]] {
            let f = Fixture::new();
            fs::write(f.root.path().join(NAME), impostor).unwrap();
            f.refuse();
        }
    }

    #[test]
    fn patched_inventory_binds_metadata_role_and_held_namespace() {
        let fixture = Fixture::with_payload("qemu-img.exe", BYTES);
        let pin = |bytes: &[u8]| ArtifactPin {
            sha256: Sha256::digest(bytes).into(),
            size: bytes.len() as u64,
        };
        let manifest = pin(fixture.manifest.as_bytes());
        let receipt = pin(b"receipt");
        let image = pin(BYTES);
        let inspect = |manifest, receipt, image| {
            collect_exact(
                &fixture.path,
                manifest,
                receipt,
                1,
                &[("qemu-img.exe", image)],
            )
        };
        let (_, held) = inspect(manifest, receipt, image).unwrap();
        let path = fixture.root.path().join("qemu-img.exe");
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::rename(&path, fixture.root.path().join("other")).is_err());
        drop(held);
        assert!(inspect(pin(b"wrong manifest"), receipt, image).is_err());
        assert!(inspect(manifest, pin(b"changed"), image).is_err());
        assert!(inspect(manifest, receipt, pin(b"wrong image")).is_err());
        assert!(
            inspect(
                manifest,
                receipt,
                ArtifactPin {
                    size: image.size + 1,
                    ..image
                }
            )
            .is_err()
        );
        let wrong_case = fixture.root.path().join("QEMU-IMG.EXE");
        let intermediate = fixture.root.path().join("intermediate.exe");
        fs::rename(&path, &intermediate).unwrap();
        fs::rename(&intermediate, &wrong_case).unwrap();
        assert!(inspect(manifest, receipt, image).is_err());
        fs::rename(&wrong_case, &intermediate).unwrap();
        fs::rename(&intermediate, &path).unwrap();
        let extra = fixture.root.path().join("extra.dll");
        fs::write(&extra, BYTES).unwrap();
        assert!(inspect(manifest, receipt, image).is_err());
        fs::remove_file(extra).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::hard_link(&path, outside.path().join("alias.exe")).unwrap();
        assert!(inspect(manifest, receipt, image).is_err());
    }

    #[test]
    fn inventory_refuses_a_junction_even_when_its_target_bytes_match() {
        let f = Fixture::new();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("Firmware,ROM.bin"), BYTES).unwrap();
        let junction = f.root.path().join("share");
        fs::remove_file(f.root.path().join(NAME)).unwrap();
        fs::remove_dir(&junction).unwrap();
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(target.path())
            .output()
            .unwrap();
        assert!(result.status.success());
        f.refuse();
        fs::remove_dir(junction).unwrap();
    }

    #[test]
    fn held_payload_blocks_replacement_and_recheck_notices_new_names() {
        let f = Fixture::new();
        let deadline = || Instant::now() + INVENTORY_TIME;
        let held = f.inspect(deadline()).unwrap();
        let listed = f.root.path().join(NAME);
        let extra = f.root.path().join("share/new.bin");
        assert!(fs::remove_file(&listed).is_err());
        assert!(OpenOptions::new().write(true).open(&listed).is_err());
        fs::write(&extra, b"replacement attempt").unwrap();
        assert!(fs::rename(&extra, &listed).is_err());
        assert_eq!(held.check(deadline()), Err(REFUSAL));
        fs::remove_file(&extra).unwrap();
        assert_eq!(held.check(deadline()), Ok(()));
        drop(held);
        fs::write(&extra, b"replacement control").unwrap();
        fs::rename(extra, listed).unwrap();
        f.refuse();
    }

    #[test]
    fn limits_refuse_oversized_payload_manifest_and_elapsed_inventory() {
        let f = Fixture::new();
        assert_eq!(f.inspect(Instant::now()).err(), Some(REFUSAL));
        let deadline = Instant::now() + INVENTORY_TIME;
        let mut bundle = Bundle::open(&f.path).unwrap();
        assert_eq!(
            bundle.payload(&[0; MAX_MANIFEST_BYTES + 1], 1, deadline, &[]),
            Err(REFUSAL)
        );
        assert!(parse_manifest(f.manifest.as_bytes(), PAYLOAD_ENTRIES + 1).is_none());
        OpenOptions::new()
            .write(true)
            .open(f.root.path().join(NAME))
            .unwrap()
            .set_len(MAX_PAYLOAD_FILE + 1)
            .unwrap();
        f.refuse();
    }
}
