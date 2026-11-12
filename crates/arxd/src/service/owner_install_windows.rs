//! Protected-state provisioning only. Registration is the final operation.
#![allow(dead_code)] // Owner CLI wiring is a later, independently reviewed slice.
use super::owner_windows::Failure;
use super::{
    layout::{self, Error, Platform, Policy},
    native,
    owner_scm::{Clock, Observation, ScmCalls, WindowsScm},
    owner_windows::{
        OpenError, OwnerGuard, ProtectionPolicy, descriptor, enable_restore, pin_chain, structural,
    },
};
use std::{
    fs::File,
    io::Read,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    path::{Path, PathBuf},
    ptr::null_mut,
};
use windows_sys::Win32::{Foundation::*, Security::SECURITY_ATTRIBUTES, Storage::FileSystem::*};

const READ_ONLY: u32 =
    READ_CONTROL | FILE_READ_DATA | FILE_READ_ATTRIBUTES | GENERIC_READ | SYNCHRONIZE;
const READ: u32 = READ_CONTROL | FILE_READ_DATA | FILE_READ_ATTRIBUTES;
const MEMBERS: [&str; 3] = [
    "activity.lock",
    "activity.sqlite3",
    "activity.sqlite3-journal",
];

/// Consumes binary authority; does not configure or activate the service.
pub fn install(guard: OwnerGuard) -> Result<(), OpenError> {
    let mut scm = WindowsScm::system(guard.layout())?;
    install_with(
        &mut scm,
        Namespace::Production(guard),
        &ProtectionPolicy::production(),
        enable_restore,
    )
}

enum Namespace {
    Production(OwnerGuard),
    #[cfg(test)]
    Fixture {
        parent: PathBuf,
        binary_directory: File,
    },
}
impl Namespace {
    fn binary_check(&self, policy: &ProtectionPolicy) -> Result<(), OpenError> {
        let f = match self {
            Self::Production(g) => g.binary_directory()?,
            #[cfg(test)]
            Self::Fixture {
                binary_directory, ..
            } => {
                structural(binary_directory, true)?;
                return policy
                    .check(binary_directory, true)
                    .map_err(|_| Error::Permissions.into());
            }
        };
        let _ = policy;
        // BinaryIntegrity is an ACL policy for both image and directory. The
        // directory must not be sent through the image's file-type check.
        structural(f, true)?;
        layout::check_protection(
            Platform::Windows,
            Policy::BinaryIntegrity,
            true,
            &native::windows_protection(f)?,
        )?;
        Ok(())
    }
    fn pin(self) -> Result<(PathBuf, Vec<File>), OpenError> {
        match self {
            Self::Production(g) => {
                let parent = g
                    .layout()
                    .state()
                    .parent()
                    .and_then(Path::parent)
                    .ok_or(Error::Layout)?
                    .to_owned();
                let mut pins = g.into_install_pins();
                pins.extend(pin_chain(&parent, None)?);
                Ok((parent, pins))
            }
            #[cfg(test)]
            Self::Fixture {
                parent,
                binary_directory,
            } => {
                native::components(&parent, true)?;
                let f = existing(&parent, READ, FILE_SHARE_READ)?.ok_or(Error::Layout)?;
                structural(&f, true)?;
                Ok((parent, vec![binary_directory, f]))
            }
        }
    }
}

// Declaration order is deliberate: retained state handles and directory/image
// pins close before owner.lock on success and every error path after acquisition.
struct Held {
    members: Vec<File>,
    pins: Vec<File>,
    owner_lock: Option<File>,
}
fn install_with<C: ScmCalls, K: Clock>(
    scm: &mut WindowsScm<C, K>,
    ns: Namespace,
    policy: &ProtectionPolicy,
    enable: impl FnOnce() -> Result<(), Error>,
) -> Result<(), OpenError> {
    ns.binary_check(policy)?;
    match scm.observe()? {
        Observation::Absent | Observation::Ours { active: false, .. } => (),
        Observation::Foreign => return Err(Error::Foreign.into()),
        Observation::Ours { active: true, .. } => return Err(Error::Active.into()),
    }
    let (parent, pins) = ns.pin()?;
    let mut held = Held {
        members: vec![],
        pins,
        owner_lock: None,
    };
    let root = parent.join("Arxcovo");
    let config = root.join("config");
    let state = root.join("state");
    enable()?;
    held.pins.push(directory(&root, policy, FILE_SHARE_READ)?);
    held.pins.push(directory(
        &config,
        policy,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?);
    let lock_path = config.join("owner.lock");
    let lock = match create(&lock_path, policy, 0) {
        Ok(f) => f,
        Err(Failure::Conflict) => existing(&lock_path, READ, 0)?.ok_or(Failure::Io)?,
        Err(e) => return Err(e.into()),
    };
    held.owner_lock = Some(lock);
    let lock = held.owner_lock.as_ref().ok_or(Failure::Io)?;
    policy.check(lock, false)?;
    if lock.metadata().map_err(|_| Failure::Io)?.len() != 0 {
        return Err(Failure::RecoveryRequired.into());
    }
    held.pins.push(directory(&state, policy, FILE_SHARE_READ)?);
    sidefiles(&state)?;
    let mut missing = Vec::new();
    // Classify every existing member before creating any missing member.
    for (i, name) in MEMBERS.iter().enumerate() {
        let Some(mut f) = existing(
            &state.join(name),
            READ,
            if i == 0 { 0 } else { FILE_SHARE_READ },
        )?
        else {
            missing.push(i);
            continue;
        };
        policy.check(&f, false).map_err(|_| Error::Permissions)?;
        let length = f.metadata().map_err(|_| Failure::Io)?.len();
        if i == 0 && length != 0 {
            return Err(Error::Permissions.into());
        }
        if i == 1 && length != 0 {
            if length < 20 {
                return Err(Error::Permissions.into());
            }
            let mut header = [0; 20];
            f.read_exact(&mut header).map_err(|_| Failure::Io)?;
            native::rollback_database_header(&header)?;
        }
        held.members.push(f);
    }
    for i in missing {
        let f = create(
            &state.join(MEMBERS[i]),
            policy,
            if i == 0 { 0 } else { FILE_SHARE_READ },
        )?;
        held.members.push(f);
    }
    sidefiles(&state)?;
    scm.register()?;
    Ok(())
}

/// The only installer OPEN_EXISTING entry. Validate the mask before any native
/// call, including when the object is absent. Restore never grants write access.
fn existing(path: &Path, access: u32, share: u32) -> Result<Option<File>, Failure> {
    if access & !READ_ONLY != 0 {
        return Err(Failure::RecoveryRequired);
    }
    open(path, access, share, OPEN_EXISTING, None)
}
fn open(
    path: &Path,
    access: u32,
    share: u32,
    disposition: u32,
    sd: Option<&super::owner_windows::Local>,
) -> Result<Option<File>, Failure> {
    // Defense in depth: a future caller cannot bypass existing's access fence.
    if disposition == OPEN_EXISTING && access & !READ_ONLY != 0 {
        return Err(Failure::RecoveryRequired);
    }
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.map_or(null_mut(), |s| s.as_ptr()),
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            if sd.is_some() { &sa } else { std::ptr::null() },
            disposition,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return match unsafe { GetLastError() } {
            ERROR_FILE_NOT_FOUND if disposition == OPEN_EXISTING => Ok(None),
            ERROR_FILE_EXISTS if disposition == CREATE_NEW => Err(Failure::Conflict),
            ERROR_SHARING_VIOLATION => Err(Failure::Busy),
            _ => Err(Failure::Io),
        };
    }
    Ok(Some(unsafe { File::from_raw_handle(handle) }))
}
fn create(path: &Path, policy: &ProtectionPolicy, share: u32) -> Result<File, Failure> {
    let sd = descriptor(&policy.creation_descriptor())?;
    let f = open(
        path,
        GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
        share,
        CREATE_NEW,
        Some(&sd),
    )?
    .ok_or(Failure::Io)?;
    policy.check(&f, false)?;
    f.sync_all().map_err(|_| Failure::Io)?;
    #[cfg(test)]
    tests::created(path)?;
    Ok(f)
}
fn directory(path: &Path, policy: &ProtectionPolicy, share: u32) -> Result<File, OpenError> {
    let sd = descriptor(&policy.creation_descriptor())?;
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_ptr(),
        bInheritHandle: 0,
    };
    let made = unsafe { CreateDirectoryW(name.as_ptr(), &sa) } != 0;
    if !made && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Err(Failure::Io.into());
    }
    let f = existing(path, READ, share)?.ok_or(Failure::Io)?;
    policy.check(&f, true).map_err(|_| Error::Permissions)?;
    #[cfg(test)]
    if made {
        tests::created(path)?;
    }
    Ok(f)
}
fn sidefiles(state: &Path) -> Result<(), OpenError> {
    for name in ["activity.sqlite3-wal", "activity.sqlite3-shm"] {
        if !matches!(existing(&state.join(name), READ, FILE_SHARE_READ), Ok(None)) {
            return Err(Error::Permissions.into());
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "owner_install_windows_tests.rs"]
mod tests;
