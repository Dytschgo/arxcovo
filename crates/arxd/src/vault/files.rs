//! Bind the retained state file and SQLite's actual main file before any SQL.
use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags, ffi};
use std::{fs, path::Path};

pub(super) fn open(dir: &Path) -> Result<(Connection, fs::File, fs::File)> {
    crate::config::checked_path(dir)?;
    crate::security::private(dir)?;
    ensure!(dir.is_dir(), "vault state directory required");
    let lock_path = dir.join("vault.lock");
    check_file(&lock_path)?;
    let lock = options().write(true).create(true).open(&lock_path)?;
    lock.try_lock()?;
    verify_path(&lock, &lock_path)?;

    let path = dir.join("vault.sqlite3");
    for suffix in ["-journal", "-wal", "-shm"] {
        let exists = check_file(&dir.join(format!("vault.sqlite3{suffix}")))?;
        ensure!(!exists || suffix == "-journal", "unsupported vault sidecar");
    }
    let file = if check_file(&path)? {
        options().open(&path)?
    } else {
        options().write(true).create_new(true).open(&path)?
    };
    verify_path(&file, &path)?;
    let mut db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    ensure!(
        sqlite_identity(&mut db)? == identity(&file)?,
        "vault database changed"
    );
    verify_path(&file, &path)?;
    Ok((db, file, lock))
}

fn check_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            crate::config::checked_path(path)?;
            ensure!(meta.is_file(), "regular vault file required");
            crate::security::private(path)?;
            identity(&options().open(path)?)?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn verify_path(file: &fs::File, path: &Path) -> Result<()> {
    ensure!(check_file(path)?, "vault state file disappeared");
    ensure!(
        identity(file)? == identity(&options().open(path)?)?,
        "vault state file changed"
    );
    Ok(())
}

fn options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

#[cfg(unix)]
fn identity(file: &fs::File) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.nlink() == 1,
        "single-link vault file required"
    );
    Ok((meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn identity(file: &fs::File) -> Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    windows_identity(file.as_raw_handle())
}

#[cfg(windows)]
fn windows_identity(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<(u64, u64)> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    };
    ensure!(
        !handle.is_null() && handle != INVALID_HANDLE_VALUE,
        "invalid vault handle"
    );
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // The caller holds the borrowed handle for the duration of this query.
    ensure!(
        unsafe {
            GetFileType(handle) == FILE_TYPE_DISK
                && GetFileInformationByHandle(handle, &mut info) != 0
        } && info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
            && info.nNumberOfLinks == 1,
        "single-link vault file required"
    );
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

fn sqlite_identity(db: &mut Connection) -> Result<(u64, u64)> {
    // ADR 0035 requires the pinned library, native VFS and exact main-file methods.
    let handle = unsafe { db.handle() };
    ensure!(
        unsafe {
            ffi::sqlite3_libversion_number() == 3050002
                && std::ffi::CStr::from_ptr(ffi::sqlite3_sourceid()).to_bytes()
                    == b"2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079"
                && ffi::sqlite3_arxcovo_bound_abi_v1() == 1
                && ffi::sqlite3_arxcovo_qualify_main_v1(handle) == ffi::SQLITE_OK
        },
        "unqualified SQLite vault main database"
    );
    #[cfg(unix)]
    {
        const MAGIC: u32 = 0x41525849;
        const _: () = assert!(size_of::<ffi::ArxIdentityV1>() == 80);
        let mut identity = ffi::ArxIdentityV1 {
            magic: MAGIC,
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
        };
        // The pinned extension fstats the actual descriptor into this versioned result.
        let rc = unsafe {
            ffi::sqlite3_file_control(
                handle,
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut identity).cast(),
            )
        };
        let vfs_kind = if cfg!(target_os = "macos") { 2 } else { 1 };
        ensure!(
            rc == ffi::SQLITE_OK
                && identity.magic == MAGIC
                && identity.abi_version == 1
                && identity.struct_bytes == 80
                && identity.status == 0
                && identity.os_errno == 0
                && identity.vfs_kind == vfs_kind
                && identity.nlink_lo == 1
                && identity.nlink_hi == 0
                && identity.mode & 0o170077 == 0o100000
                && identity.flags == 0
                && identity.reserved == [0; 6],
            "unavailable SQLite vault identity"
        );
        Ok((
            (u64::from(identity.dev_hi) << 32) | u64::from(identity.dev_lo),
            (u64::from(identity.ino_hi) << 32) | u64::from(identity.ino_lo),
        ))
    }
    #[cfg(windows)]
    {
        let mut borrowed: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                handle,
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                std::ptr::from_mut(&mut borrowed).cast(),
            )
        };
        ensure!(rc == ffi::SQLITE_OK, "unavailable SQLite vault handle");
        windows_identity(borrowed)
    }
}
