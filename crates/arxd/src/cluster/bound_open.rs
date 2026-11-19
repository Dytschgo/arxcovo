//! First-slice low-level proof only. No production caller or store construction.
#![cfg(test)]

use rusqlite::{Connection, ffi};
use std::{ffi::CStr, fs::File, path::Path};

type IdentityAbi = ffi::ArxIdentityV1;
const _: () = {
    assert!(size_of::<IdentityAbi>() == 80);
    assert!(align_of::<IdentityAbi>() == 4);
    assert!(std::mem::offset_of!(IdentityAbi, magic) == 0);
    assert!(std::mem::offset_of!(IdentityAbi, abi_version) == 4);
    assert!(std::mem::offset_of!(IdentityAbi, struct_bytes) == 8);
    assert!(std::mem::offset_of!(IdentityAbi, status) == 12);
    assert!(std::mem::offset_of!(IdentityAbi, os_errno) == 16);
    assert!(std::mem::offset_of!(IdentityAbi, vfs_kind) == 20);
    assert!(std::mem::offset_of!(IdentityAbi, dev_lo) == 24);
    assert!(std::mem::offset_of!(IdentityAbi, dev_hi) == 28);
    assert!(std::mem::offset_of!(IdentityAbi, ino_lo) == 32);
    assert!(std::mem::offset_of!(IdentityAbi, ino_hi) == 36);
    assert!(std::mem::offset_of!(IdentityAbi, nlink_lo) == 40);
    assert!(std::mem::offset_of!(IdentityAbi, nlink_hi) == 44);
    assert!(std::mem::offset_of!(IdentityAbi, mode) == 48);
    assert!(std::mem::offset_of!(IdentityAbi, flags) == 52);
    assert!(std::mem::offset_of!(IdentityAbi, reserved) == 56);
};

#[derive(Debug, PartialEq, Eq)]
struct ObjectId(u64, u64);

fn initial() -> IdentityAbi {
    IdentityAbi {
        magic: 0x41525849,
        abi_version: 1,
        struct_bytes: 80,
        status: u32::MAX,
        os_errno: 0,
        vfs_kind: 0,
        dev_lo: 0,
        dev_hi: 0,
        ino_lo: 0,
        ino_hi: 0,
        nlink_lo: 0,
        nlink_hi: 0,
        mode: 0,
        flags: 0,
        reserved: [0; 6],
    }
}
fn words(lo: u32, hi: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

// Format validation is deliberately distinct from real native identity evidence.
fn decode(rc: i32, a: &IdentityAbi, kind: u32) -> Result<ObjectId, ()> {
    if rc != ffi::SQLITE_OK
        || a.magic != 0x41525849
        || a.abi_version != 1
        || a.struct_bytes != 80
        || a.status != 0
        || a.os_errno != 0
        || a.vfs_kind != kind
        || !(1..=2).contains(&kind)
        || a.flags != 0
        || a.reserved != [0; 6]
        || words(a.nlink_lo, a.nlink_hi) != 1
        || a.mode & 0o170000 != 0o100000
        || a.mode & 0o077 != 0
    {
        return Err(());
    }
    Ok(ObjectId(
        words(a.dev_lo, a.dev_hi),
        words(a.ino_lo, a.ino_hi),
    ))
}

fn qualify(db: &mut Connection) -> Result<(), ()> {
    // Exclusive borrow encloses all public SQLite handle operations. No callbacks/SQL.
    unsafe {
        if ffi::sqlite3_libversion_number() != 3050002
            || CStr::from_ptr(ffi::sqlite3_sourceid()).to_bytes() != b"2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079"
            || ffi::sqlite3_arxcovo_bound_abi_v1() != 1
            || ffi::sqlite3_arxcovo_qualify_main_v1(db.handle()) != ffi::SQLITE_OK {
            return Err(());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn guard_id(file: &File) -> Result<ObjectId, ()> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().map_err(|_| ())?;
    if !m.is_file()
        || m.nlink() != 1
        || m.mode() & 0o077 != 0
        || m.uid() != unsafe { libc::geteuid() }
    {
        return Err(());
    }
    // MetadataExt on macOS exposes signed dev_t through u64; reject negative origin.
    #[cfg(target_os = "macos")]
    if m.dev() > i32::MAX as u64 {
        return Err(());
    }
    Ok(ObjectId(m.dev(), m.ino()))
}
#[cfg(windows)]
fn windows_id(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<ObjectId, ()> {
    use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(());
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        if GetFileType(handle) != FILE_TYPE_DISK
            || GetFileInformationByHandle(handle, &mut info) == 0
        {
            return Err(());
        }
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(());
    }
    Ok(ObjectId(
        u64::from(info.dwVolumeSerialNumber),
        words(info.nFileIndexLow, info.nFileIndexHigh),
    ))
}
#[cfg(windows)]
fn guard_id(file: &File) -> Result<ObjectId, ()> {
    use std::os::windows::io::AsRawHandle;
    windows_id(file.as_raw_handle())
}

fn actual_id(db: &mut Connection) -> Result<ObjectId, ()> {
    qualify(db)?;
    #[cfg(unix)]
    {
        let mut a = initial();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut a).cast(),
            )
        };
        let kind = if cfg!(target_os = "macos") { 2 } else { 1 };
        decode(rc, &a, kind)
    }
    #[cfg(windows)]
    {
        let mut borrowed: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                29,
                std::ptr::from_mut(&mut borrowed).cast(),
            )
        };
        if rc != ffi::SQLITE_OK {
            return Err(());
        }
        // Borrowed only for this measurement; never close, adopt or return HANDLE.
        windows_id(borrowed)
    }
}

fn verify(db: &mut Connection, original: &File, path: &Path) -> Result<ObjectId, ()> {
    crate::config::checked_path(path).map_err(|_| ())?;
    crate::security::private(path).map_err(|_| ())?;
    crate::security::private(path.parent().ok_or(())?).map_err(|_| ())?;
    let before = guard_id(original)?;
    let actual = actual_id(db)?;
    if actual != before || guard_id(original)? != before {
        return Err(());
    }
    // Recheck path object without treating it as SQLite identity proof.
    if guard_id(&File::open(path).map_err(|_| ())?)? != before {
        return Err(());
    }
    crate::config::checked_path(path).map_err(|_| ())?;
    crate::security::private(path).map_err(|_| ())?;
    Ok(actual)
}

mod tests {
    use super::*;
    use std::fs;
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, File) {
        let dir = crate::service::application::tests::private_temp();
        let p = dir.path().join("proof.sqlite3");
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&p)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        (dir, p, f)
    }
    fn open(p: &Path) -> Connection {
        Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
    }
    #[test]
    fn abi_format_refusals_are_not_identity_acceptance() {
        let a = initial();
        assert!(decode(0, &a, 1).is_err());
        let mut valid = initial();
        valid.status = 0;
        valid.vfs_kind = 1;
        valid.nlink_lo = 1;
        valid.mode = 0o100600;
        assert_eq!(decode(0, &valid, 1), Ok(ObjectId(0, 0)));
        for index in 0..14 {
            let mut b = valid;
            match index {
                0 => b.magic = 0,
                1 => b.abi_version = 2,
                2 => b.struct_bytes = 79,
                3 => b.status = 1,
                4 => b.status = 2,
                5 => b.status = 3,
                6 => b.status = u32::MAX,
                7 => b.os_errno = 1,
                8 => b.vfs_kind = 3,
                9 => b.flags = 1,
                10 => b.reserved[5] = 1,
                11 => b.nlink_lo = 2,
                12 => b.mode = 0o040700,
                _ => b.mode = 0o100644,
            }
            assert!(decode(0, &b, 1).is_err(), "field {index}");
        }
        assert!(decode(ffi::SQLITE_NOTFOUND, &valid, 1).is_err());
        assert!(decode(0, &valid, 0).is_err());
        assert_eq!(words(u32::MAX, 0x80000000), 0x80000000ffffffff);
    }
    #[test]
    fn stable_actual_connection_identity() {
        let (_d, p, f) = fixture();
        let mut db = open(&p);
        let pointer = unsafe { db.handle() };
        let id = verify(&mut db, &f, &p).unwrap();
        assert_eq!(id, guard_id(&f).unwrap());
        assert_eq!(verify(&mut db, &f, &p).unwrap(), id);
        assert_eq!(unsafe { db.handle() }, pointer);
    }
    #[test]
    fn original_guard_vs_other_actual_sqlite_file_refuses() {
        let (_a, p, f) = fixture();
        let (_b, q, g) = fixture();
        assert_ne!(guard_id(&f).unwrap(), guard_id(&g).unwrap());
        let mut db = open(&q);
        assert!(verify(&mut db, &f, &p).is_err());
        assert!(verify(&mut db, &g, &q).is_ok());
    }
    #[test]
    fn real_hardlink_refuses() {
        let (d, p, f) = fixture();
        let mut db = open(&p);
        assert!(verify(&mut db, &f, &p).is_ok());
        fs::hard_link(&p, d.path().join("alias")).unwrap();
        assert!(verify(&mut db, &f, &p).is_err());
    }
    #[test]
    fn real_directory_type_refuses() {
        let (d, _, _) = fixture();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            };
            options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let directory = options.open(d.path()).unwrap();
        assert!(guard_id(&directory).is_err());
    }
    #[test]
    fn read_only_actual_connection_identity() {
        let (_d, p, f) = fixture();
        let mut db =
            Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let pointer = unsafe { db.handle() };
        assert_eq!(verify(&mut db, &f, &p).unwrap(), guard_id(&f).unwrap());
        assert_eq!(unsafe { db.handle() }, pointer);
        assert_eq!(fs::read(&p).unwrap(), b"");
    }
    #[test]
    fn memory_vfs_and_unknown_opcode_refuse() {
        let mut memory = Connection::open_in_memory().unwrap();
        assert!(qualify(&mut memory).is_err());
        let (_d, p, _) = fixture();
        let db = open(&p);
        let mut a = initial();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                0x41525832,
                std::ptr::from_mut(&mut a).cast(),
            )
        };
        assert_eq!(rc, ffi::SQLITE_NOTFOUND);
        assert_eq!(a.status, u32::MAX);
    }
    #[cfg(unix)]
    #[test]
    fn actual_unix_swap_and_privacy_refuse() {
        use std::os::unix::fs::PermissionsExt;
        let (d, p, f) = fixture();
        fs::rename(&p, d.path().join("original")).unwrap();
        fs::write(&p, b"").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let mut db = open(&p);
        assert!(verify(&mut db, &f, &p).is_err());
        let current = File::open(&p).unwrap();
        assert!(verify(&mut db, &current, &p).is_ok());
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(verify(&mut db, &current, &p).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn actual_unix_abi_header_and_unsupported_alias_refuse() {
        let (_d, p, _) = fixture();
        let db = open(&p);
        let mut a = initial();
        a.magic = 0;
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut a).cast(),
            )
        };
        assert_eq!(rc, ffi::SQLITE_MISMATCH);
        assert_eq!(a.status, u32::MAX);
        let mut alias = Connection::open_with_flags_and_vfs(
            &p,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            "unix-none",
        )
        .unwrap();
        assert!(qualify(&mut alias).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_swap_denial_is_prevention_only() {
        let (d, p, f) = fixture();
        let mut db = open(&p);
        let error = fs::rename(&p, d.path().join("moved")).unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32),
            "{error:?}"
        );
        assert!(verify(&mut db, &f, &p).is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn windows_actual_alias_vfs_refuses() {
        let (_d, p, _) = fixture();
        let mut alias = Connection::open_with_flags_and_vfs(
            &p,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            "win32-longpath",
        )
        .unwrap();
        assert!(qualify(&mut alias).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_real_privacy_change_refuses() {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{Authorization::*, *},
        };
        let (_d, p, f) = fixture();
        let mut db = open(&p);
        assert!(verify(&mut db, &f, &p).is_ok());
        // Change only this disposable file to an Everyone grant; no admin needed.
        let text: Vec<u16> = "D:P(A;;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
        unsafe {
            let mut sd = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    1,
                    &mut sd,
                    ptr::null_mut()
                ),
                0
            );
            let (mut present, mut defaulted) = (0, 0);
            let mut acl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
                0
            );
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
            let status = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            );
            LocalFree(sd);
            assert_eq!(status, 0);
        }
        assert!(crate::security::private(&p).is_err());
        assert!(verify(&mut db, &f, &p).is_err());
    }
}
