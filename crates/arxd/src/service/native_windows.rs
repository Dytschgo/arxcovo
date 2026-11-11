use super::super::{
    config::AccountKind,
    layout::{Ace, Platform},
};
use super::*;
use std::{
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NONE_MAPPED, GENERIC_READ, GetLastError, INVALID_HANDLE_VALUE, LocalFree},
    NetworkManagement::NetManagement::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
};
pub const PLATFORM: Platform = Platform::Windows;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Local(*mut std::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Net(*mut u8);
impl Drop for Net {
    fn drop(&mut self) {
        unsafe {
            NetApiBufferFree(self.0.cast());
        }
    }
}
fn sid_string(p: PSID) -> Result<String, Error> {
    unsafe {
        if p.is_null() || IsValidSid(p) == 0 || GetLengthSid(p) > 68 {
            return Err(Error::Identity);
        }
        let mut out = null_mut();
        if ConvertSidToStringSidW(p, &mut out) == 0 {
            return Err(Error::Identity);
        }
        let _guard = Local(out.cast());
        string(out, 184)
    }
}
// Only native API-owned strings are accepted; each scan has a fixed cap.
fn string(p: *const u16, cap: usize) -> Result<String, Error> {
    if p.is_null() {
        return Err(Error::Identity);
    }
    unsafe {
        let mut n = 0;
        while n < cap && *p.add(n) != 0 {
            n += 1;
        }
        if n == cap {
            return Err(Error::Identity);
        }
        String::from_utf16(std::slice::from_raw_parts(p, n)).map_err(|_| Error::Identity)
    }
}
// Only an explicit native not-found result establishes absence. Access denied,
// insufficient buffers and directory-service failures establish no such fact.
fn lookup_failure(code: u32) -> Error {
    if code == ERROR_NONE_MAPPED {
        Error::AccountMissing
    } else {
        Error::Identity
    }
}
fn lookup(name: &str) -> Result<(Vec<u32>, SID_NAME_USE), Error> {
    let name = wide(name);
    let mut sid = vec![0u32; 17];
    let mut bytes = 68;
    let mut domain = vec![0u16; 257];
    let mut chars = domain.len() as u32;
    let mut kind = 0;
    if unsafe {
        LookupAccountNameW(
            null(),
            name.as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut bytes,
            domain.as_mut_ptr(),
            &mut chars,
            &mut kind,
        )
    } == 0
    {
        return Err(lookup_failure(unsafe { GetLastError() }));
    }
    Ok((sid, kind))
}
pub fn account(value: &str) -> Result<Account, Error> {
    // Resolve SID inputs to a name, then require the local SAM user to have the
    // exact same SID. Domain and service accounts cannot pass that comparison.
    let (sid, kind) = if value.starts_with("S-") {
        let mut p = null_mut();
        if unsafe { ConvertStringSidToSidW(wide(value).as_ptr(), &mut p) } == 0 {
            return Err(Error::Identity);
        }
        let _guard = Local(p);
        let mut name = vec![0u16; 257];
        let mut domain = vec![0u16; 257];
        let mut n = 257;
        let mut d = 257;
        let mut kind = 0;
        if unsafe {
            LookupAccountSidW(
                null(),
                p,
                name.as_mut_ptr(),
                &mut n,
                domain.as_mut_ptr(),
                &mut d,
                &mut kind,
            )
        } == 0
        {
            return Err(lookup_failure(unsafe { GetLastError() }));
        }
        if kind != SidTypeUser {
            return Err(Error::Identity);
        }
        let name = string(name.as_ptr(), 257)?;
        let (sid, kind) = lookup(&format!("{}\\{}", string(domain.as_ptr(), 257)?, name))?;
        (sid, (kind, name))
    } else {
        let (sid, kind) = lookup(value)?;
        (
            sid,
            (
                kind,
                value.rsplit('\\').next().ok_or(Error::Identity)?.to_owned(),
            ),
        )
    };
    let (kind, name) = kind;
    if kind != SidTypeUser {
        return Err(Error::Identity);
    }
    let identity = sid_string(sid.as_ptr() as PSID)?;
    if !identity.starts_with("S-1-5-21-") {
        return Err(Error::Identity);
    }
    let user = wide(&name);
    let mut p = null_mut();
    let status = unsafe { NetUserGetInfo(null(), user.as_ptr(), 23, &mut p) };
    let _guard = Net(p);
    if status != 0 || p.is_null() {
        return Err(Error::Identity);
    }
    let info = unsafe { &*p.cast::<USER_INFO_23>() };
    if sid_string(info.usri23_user_sid)? != identity || info.usri23_flags & UF_NORMAL_ACCOUNT == 0 {
        return Err(Error::Identity);
    }
    let mut p = null_mut();
    let mut read = 0;
    let mut total = 0;
    let status = unsafe {
        NetUserGetLocalGroups(
            null(),
            user.as_ptr(),
            0,
            LG_INCLUDE_INDIRECT,
            &mut p,
            65536,
            &mut read,
            &mut total,
        )
    };
    let _groups = Net(p);
    if status != 0 || read != total || read > 1024 || (read > 0 && p.is_null()) {
        return Err(Error::Identity);
    }
    let mut admin = false;
    for i in 0..read as usize {
        let group = unsafe { &*p.cast::<LOCALGROUP_USERS_INFO_0>().add(i) };
        let name = string(group.lgrui0_name, 257)?;
        let (sid, kind) = lookup(&name).map_err(|_| Error::Identity)?;
        if kind != SidTypeAlias {
            return Err(Error::Identity);
        }
        admin |= sid_string(sid.as_ptr() as PSID)? == "S-1-5-32-544";
    }
    Ok(Account {
        identity,
        gid: None,
        kind: AccountKind::User,
        administrative: admin,
    })
}

pub fn protection(file: &File) -> Result<Protection, Error> {
    unsafe {
        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut sd = null_mut();
        if GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut sd,
        ) != 0
        {
            return Err(Error::Permissions);
        }
        let _guard = Local(sd);
        descriptor_protection(sd)
    }
}
fn descriptor_protection(sd: PSECURITY_DESCRIPTOR) -> Result<Protection, Error> {
    unsafe {
        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        if IsValidSecurityDescriptor(sd) == 0
            || GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) == 0
            || GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) == 0
        {
            return Err(Error::Permissions);
        }
        let owner = sid_string(owner).map_err(|_| Error::Permissions)?;
        let mut control = 0;
        let mut rev = 0;
        if GetSecurityDescriptorControl(sd, &mut control, &mut rev) == 0 {
            return Err(Error::Permissions);
        }
        let mut aces = Vec::new();
        if !acl.is_null() {
            let mut info = ACL_SIZE_INFORMATION::default();
            if IsValidAcl(acl) == 0
                || GetAclInformation(
                    acl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                    std::mem::size_of_val(&info) as u32,
                    AclSizeInformation,
                ) == 0
                || info.AceCount > 1024
                || info.AclBytesInUse > 65536
            {
                return Err(Error::Permissions);
            }
            for i in 0..info.AceCount {
                let mut p = null_mut();
                if GetAce(acl, i, &mut p) == 0 {
                    return Err(Error::Permissions);
                }
                let h = &*p.cast::<ACE_HEADER>();
                aces.push(match h.AceType {
                    0 | 1 => {
                        if (h.AceSize as usize) < 16 {
                            return Err(Error::Permissions);
                        }
                        let a = &*p.cast::<ACCESS_ALLOWED_ACE>();
                        let sid = (&a.SidStart as *const u32) as PSID;
                        if IsValidSid(sid) == 0 || 8 + GetLengthSid(sid) > u32::from(h.AceSize) {
                            return Err(Error::Permissions);
                        }
                        if h.AceType == 1 {
                            Ace::Deny
                        } else {
                            Ace::Allow {
                                sid: sid_string(sid).map_err(|_| Error::Permissions)?,
                                mask: a.Mask,
                                inherit_only: h.AceFlags & 8 != 0,
                            }
                        }
                    }
                    _ => Ace::Unsupported,
                });
            }
        }
        Ok(Protection::Windows {
            owner,
            protected: control & SE_DACL_PROTECTED != 0,
            dacl_present: control & SE_DACL_PRESENT != 0 && !acl.is_null(),
            aces,
        })
    }
}
pub fn open(path: &Path, directory: bool, read: bool, trusted: bool) -> Result<Observed, Error> {
    open_impl(path, directory, read, trusted, false)
}
pub fn open_state_file(path: &Path) -> Result<Observed, Error> {
    open_impl(path, false, false, true, true)
}
#[cfg(test)]
pub(super) fn open_state_fixture(path: &Path, directory: bool) -> Result<Observed, Error> {
    // Same native access/sharing and structural checks; owned-temp ancestry is
    // deliberately not asserted to be SYSTEM-owned. No runtime policy switch.
    open_impl(path, directory, false, false, !directory)
}
fn open_impl(
    path: &Path,
    directory: bool,
    read: bool,
    trusted: bool,
    mutable_leaf: bool,
) -> Result<Observed, Error> {
    let parts = components(path, true)?;
    let text = path.to_str().ok_or(Error::Layout)?;
    let mut current = std::path::PathBuf::from(format!("{}\\", &text[..2]));
    let root = wide(current.to_str().ok_or(Error::Layout)?);
    if unsafe { GetDriveTypeW(root.as_ptr()) } != 3 {
        return Err(Error::Layout);
    } // DRIVE_FIXED
    let mut ancestors = Vec::new();
    for i in 0..=parts.len() {
        if i > 0 {
            current.push(&parts[i - 1]);
        }
        let leaf = i == parts.len();
        let name: Vec<u16> = current.as_os_str().encode_wide().chain(Some(0)).collect();
        // FILE_READ_DATA (FILE_LIST_DIRECTORY on directories) makes the
        // share restrictions effective: metadata-only access does not prevent
        // rename. Mutable DB leaves permit writes, but never delete sharing.
        // An existing conflicting writer causes refusal on immutable objects.
        let access = READ_CONTROL
            | FILE_READ_ATTRIBUTES
            | FILE_READ_DATA
            | if leaf && read { GENERIC_READ } else { 0 };
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ
                    | if leaf && mutable_leaf {
                        FILE_SHARE_WRITE
                    } else {
                        0
                    },
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(Error::Layout);
        }
        let file = unsafe { File::from_raw_handle(h) };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(h, &mut info) } == 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != (!leaf || directory)
            || (leaf && !directory && info.nNumberOfLinks != 1)
        {
            return Err(Error::Layout);
        }
        let p = protection(&file)?;
        if trusted && !leaf {
            super::super::layout::check_windows_ancestor(&p)?;
        }
        if leaf {
            return Ok(Observed {
                file,
                _ancestors: ancestors,
                protection: p,
            });
        }
        ancestors.push(file);
    }
    Err(Error::Layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_lookup_errors_only_confirm_explicit_absence() {
        use windows_sys::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_SUCH_DOMAIN,
        };
        assert_eq!(lookup_failure(ERROR_NONE_MAPPED), Error::AccountMissing);
        for code in [
            0,
            ERROR_ACCESS_DENIED,
            ERROR_INSUFFICIENT_BUFFER,
            ERROR_NO_SUCH_DOMAIN,
            u32::MAX,
        ] {
            assert_eq!(lookup_failure(code), Error::Identity);
        }
    }

    #[test]
    fn reparse_leaf_and_ancestor_refuse_on_local_junction() {
        use std::os::windows::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("target")).unwrap();
        std::fs::write(tmp.path().join("target/file"), b"fixture").unwrap();
        let cmd = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        assert!(
            std::process::Command::new(cmd)
                .current_dir(tmp.path())
                .raw_arg("/d /c mklink /J link target")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(open(&tmp.path().join("target"), true, false, false).is_ok());
        assert!(open(&tmp.path().join("link"), true, false, false).is_err());
        assert!(open(&tmp.path().join("link/file"), false, true, false).is_err());
        std::fs::remove_dir(tmp.path().join("link")).unwrap();
    }
    #[test]
    fn native_access_checks_distinguish_creation_inheritance_and_control() {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        unsafe {
            let mut primary = null_mut();
            assert_ne!(
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_QUERY | TOKEN_DUPLICATE,
                    &mut primary
                ),
                0
            );
            let mut token = null_mut();
            let duplicated = DuplicateToken(primary, SecurityImpersonation, &mut token);
            CloseHandle(primary);
            assert_ne!(duplicated, 0);
            // Entirely in-memory descriptors: no ACL or owner changes on disk.
            for (ace, allowed, ancestor_safe) in [
                ("(A;;0x00000006;;;WD)", 6, true),
                ("(A;OICIIO;FA;;;WD)", 0, true),
                ("(A;;0x00000040;;;WD)", 0x40, false),
                ("(A;;SD;;;WD)", DELETE, false),
                ("(A;;WD;;;WD)", WRITE_DAC, false),
                ("(A;;WO;;;WD)", WRITE_OWNER, false),
            ] {
                let mut sd = null_mut();
                assert_ne!(
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        wide(&format!("O:SYG:SYD:{ace}")).as_ptr(),
                        SDDL_REVISION_1,
                        &mut sd,
                        null_mut()
                    ),
                    0
                );
                let _sd = Local(sd);
                let observed = descriptor_protection(sd).unwrap();
                assert_eq!(
                    super::super::super::layout::check_windows_ancestor(&observed).is_ok(),
                    ancestor_safe
                );
                for desired in [
                    FILE_ADD_FILE,
                    FILE_ADD_SUBDIRECTORY,
                    FILE_DELETE_CHILD,
                    DELETE,
                    WRITE_DAC,
                    WRITE_OWNER,
                ] {
                    let mapping = GENERIC_MAPPING {
                        GenericRead: FILE_GENERIC_READ,
                        GenericWrite: FILE_GENERIC_WRITE,
                        GenericExecute: FILE_GENERIC_EXECUTE,
                        GenericAll: FILE_ALL_ACCESS,
                    };
                    let mut privileges = PRIVILEGE_SET::default();
                    let mut size = std::mem::size_of_val(&privileges) as u32;
                    let mut granted = 0;
                    let mut status = 0;
                    assert_ne!(
                        AccessCheck(
                            sd,
                            token,
                            desired,
                            &mapping,
                            &mut privileges,
                            &mut size,
                            &mut granted,
                            &mut status
                        ),
                        0
                    );
                    assert_eq!(status != 0, allowed & desired != 0, "{ace}: {desired:x}");
                }
            }
            CloseHandle(token);
        }
    }
    #[test]
    fn real_current_user_is_local_or_refused_without_disclosure() {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        unsafe {
            let mut token = null_mut();
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                0
            );
            let mut buf = vec![0u64; 1024];
            let mut needed = 0;
            let ok =
                GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), 8192, &mut needed);
            CloseHandle(token);
            assert_ne!(ok, 0);
            let user = &*buf.as_ptr().cast::<TOKEN_USER>();
            let pin = sid_string(user.User.Sid).unwrap();
            let mut name = vec![0u16; 257];
            let mut domain = vec![0u16; 257];
            let mut n = 257;
            let mut d = 257;
            let mut kind = 0;
            assert_ne!(
                LookupAccountSidW(
                    null(),
                    user.User.Sid,
                    name.as_mut_ptr(),
                    &mut n,
                    domain.as_mut_ptr(),
                    &mut d,
                    &mut kind
                ),
                0
            );
            let mut local = null_mut();
            let status = NetUserGetInfo(null(), name.as_ptr(), 23, &mut local);
            let _guard = Net(local);
            if status == 0 && !local.is_null() {
                let info = &*local.cast::<USER_INFO_23>();
                if sid_string(info.usri23_user_sid).unwrap() == pin
                    && info.usri23_flags & UF_NORMAL_ACCOUNT != 0
                {
                    // Positive control when this runner really is a local SAM
                    // user. A resolver that always refuses cannot pass here.
                    assert!(account(&pin).is_ok());
                }
            }
            match account(&pin) {
                Ok(a) => {
                    assert!(a.identity == pin);
                    assert_eq!(a.kind, AccountKind::User);
                    assert_eq!(a.gid, None);
                }
                Err(e) => assert_eq!(e, Error::Identity),
            }
            assert!(account("S-1-5-18").is_err());
            assert!(account("S-1-5-32-544").is_err());
        }
    }
    #[test]
    fn current_user_file_never_satisfies_system_private_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test");
        std::fs::write(&path, b"not-config").unwrap();
        let opened = open(&path, false, true, false).unwrap();
        assert!(check_protection(PLATFORM, Policy::PrivateFile, true, &opened.protection).is_err());
        assert!(checked(&path, false, Policy::PrivateFile, true).is_err());
    }
    #[test]
    fn opened_objects_exclude_writers_and_rename_until_released() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test");
        std::fs::write(&path, b"fixture").unwrap();
        let opened = open(&path, false, true, false).unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::rename(&path, tmp.path().join("renamed")).is_err());
        drop(opened);
        std::fs::rename(&path, tmp.path().join("renamed")).unwrap();
    }
    #[test]
    fn hardlinked_file_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test");
        std::fs::write(&path, b"fixture").unwrap();
        let alias = tmp.path().join("alias");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(open(&path, false, true, false).is_err());
        assert!(open(&alias, false, true, false).is_err());
    }
}

#[cfg(test)]
mod mutable_pin_tests {
    use super::*;
    #[test]
    fn mutable_leaf_allows_database_writes_but_blocks_namespace_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        std::fs::write(&path, []).unwrap();
        // Explicit untrusted fixture; production open_state_file requires
        // trusted root/SYSTEM ancestry and the caller checks PrivateFile.
        let pin = open_impl(&path, false, false, false, true).unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_ok());
        assert!(std::fs::rename(&path, dir.path().join("moved")).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        drop(pin);
        std::fs::rename(&path, dir.path().join("moved")).unwrap();
    }
}
