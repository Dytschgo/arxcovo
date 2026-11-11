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
    Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE, LocalFree},
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
        return Err(Error::Identity);
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
            || kind != SidTypeUser
        {
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
        let (sid, kind) = lookup(&name)?;
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
                                writes: a.Mask & (0x10000000 | 0x40000000 | 0x000d0156) != 0,
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
        // Denying write/delete sharing pins the checked ancestry until the
        // final read finishes. An existing conflicting writer causes refusal.
        let access =
            READ_CONTROL | FILE_READ_ATTRIBUTES | if leaf && read { GENERIC_READ } else { 0 };
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ,
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
            check_protection(PLATFORM, Policy::BinaryIntegrity, true, &p)?;
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
