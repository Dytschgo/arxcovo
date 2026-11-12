use super::{ClientError, ClientHandle, PostCreateOps};
use std::{
    ffi::c_void,
    fs::File,
    io::{self, Write},
    mem::MaybeUninit,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::*,
};
fn fail() -> io::Error {
    io::Error::other("client file operation")
}
fn bool_result(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
unsafe fn sid_text(sid: PSID) -> io::Result<String> {
    unsafe {
        if sid.is_null() || IsValidSid(sid) == 0 {
            return Err(fail());
        }
        let mut text = ptr::null_mut();
        bool_result(ConvertSidToStringSidW(sid, &mut text))?;
        let _guard = Local(text.cast());
        let mut n = 0;
        while n < 184 && *text.add(n) != 0 {
            n += 1;
        }
        if n == 184 {
            return Err(fail());
        }
        String::from_utf16(std::slice::from_raw_parts(text, n)).map_err(|_| fail())
    }
}
fn token() -> io::Result<OwnedHandle> {
    let mut raw = ptr::null_mut();
    bool_result(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) })?;
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn user(token: &OwnedHandle) -> io::Result<String> {
    unsafe {
        let mut size = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut size,
        );
        if size == 0 || size > 65536 {
            return Err(fail());
        }
        let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        bool_result(GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        ))?;
        sid_text((*data.as_ptr().cast::<TOKEN_USER>()).User.Sid)
    }
}
pub(super) fn elevated() -> Result<bool, ClientError> {
    (|| {
        let t = token()?;
        let u = user(&t)?;
        let mut elevation = MaybeUninit::<TOKEN_ELEVATION>::uninit();
        let mut len = 0;
        bool_result(unsafe {
            GetTokenInformation(
                t.as_raw_handle(),
                TokenElevation,
                elevation.as_mut_ptr().cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
        })?;
        Ok(unsafe { elevation.assume_init() }.TokenIsElevated != 0
            || ["S-1-5-18", "S-1-5-19", "S-1-5-20"].contains(&u.as_str()))
    })()
    .map_err(|_: io::Error| ClientError::Io)
}
struct Descriptor(Local);
impl Descriptor {
    fn new(user: &str, directory: bool) -> io::Result<Self> {
        let inheritance = if directory { "OICI" } else { "" };
        let sddl = wide(&format!("O:{user}D:P(A;{inheritance};FA;;;{user})"));
        let mut sd = ptr::null_mut();
        bool_result(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut sd,
                ptr::null_mut(),
            )
        })?;
        Ok(Self(Local(sd)))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: 0,
        }
    }
}
fn open_dir(path: &str) -> io::Result<OwnedHandle> {
    let h = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(h) })
    }
}
// No Debug: facts contain current-user identifiers. Fixtures use synthetic SIDs only.
#[derive(PartialEq, Eq)]
pub(super) struct Ace {
    pub(super) kind: u8,
    pub(super) recipient: String,
}
#[derive(PartialEq, Eq)]
pub(super) struct Facts {
    pub(super) owner: String,
    pub(super) directory: bool,
    pub(super) disk: bool,
    pub(super) reparse: bool,
    pub(super) links: u32,
    pub(super) length: u64,
    pub(super) dacl: Option<Vec<Ace>>,
}
fn information(handle: ClientHandle<'_>) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = MaybeUninit::uninit();
    bool_result(unsafe { GetFileInformationByHandle(handle.as_raw_handle(), info.as_mut_ptr()) })?;
    Ok(unsafe { info.assume_init() })
}
fn facts(handle: ClientHandle<'_>) -> io::Result<Facts> {
    unsafe {
        let info = information(handle)?;
        let mut owner = ptr::null_mut();
        let mut acl = ptr::null_mut();
        let mut sd = ptr::null_mut();
        let status = GetSecurityInfo(
            handle.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut sd,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _guard = Local(sd);
        let owner = sid_text(owner)?;
        let mut present = 0;
        let mut defaulted = 0;
        bool_result(GetSecurityDescriptorDacl(
            sd,
            &mut present,
            &mut acl,
            &mut defaulted,
        ))?;
        let dacl = if present == 0 || acl.is_null() {
            None
        } else {
            let mut entries = Vec::new();
            for i in 0..(*acl).AceCount {
                let mut entry = ptr::null_mut();
                bool_result(GetAce(acl, i as u32, &mut entry))?;
                let header = &*entry.cast::<ACE_HEADER>();
                // Unsupported ACEs retain their actual type; never treat a deny
                // or unknown entry as an allowed recipient.
                let recipient = if header.AceType == 0
                    && header.AceSize as usize >= size_of::<ACCESS_ALLOWED_ACE>()
                {
                    let allow = &*entry.cast::<ACCESS_ALLOWED_ACE>();
                    sid_text(ptr::addr_of!(allow.SidStart) as PSID)?
                } else {
                    String::new()
                };
                entries.push(Ace {
                    kind: header.AceType,
                    recipient,
                });
            }
            Some(entries)
        };
        Ok(Facts {
            owner,
            directory: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
            disk: GetFileType(handle.as_raw_handle()) == FILE_TYPE_DISK,
            reparse: info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
            links: info.nNumberOfLinks,
            length: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
            dacl,
        })
    }
}
pub(super) fn check(f: &Facts, user: &str, directory: bool, expected: u64) -> io::Result<()> {
    if f.owner != user
        || f.directory != directory
        || !f.disk
        || f.reparse
        || (!directory && (f.links != 1 || f.length != expected))
    {
        return Err(fail());
    }
    let acl = f.dacl.as_ref().ok_or_else(fail)?;
    for ace in acl {
        if ace.kind != 0
            || (ace.recipient != user
                && ace.recipient != "S-1-5-18"
                && ace.recipient != "S-1-5-32-544")
        {
            return Err(fail());
        }
    }
    Ok(())
}
pub(super) struct Parent {
    _pins: Vec<OwnedHandle>,
    path: String,
    user: String,
}
impl Parent {
    pub(super) fn open(out: &str) -> Result<Self, ClientError> {
        let user =
            user(&token().map_err(|_| ClientError::Parent)?).map_err(|_| ClientError::Parent)?;
        if unsafe { GetDriveTypeW(wide(&out[..3]).as_ptr()) } != 3
        /* DRIVE_FIXED; no additional Windows API feature needed */
        {
            return Err(ClientError::Parent);
        }
        let names: Vec<_> = out[3..].split('\\').collect();
        let mut path = out[..3].to_string();
        let mut pins = vec![open_dir(&path).map_err(|_| ClientError::Parent)?];
        let root = facts(pins[0].as_handle()).map_err(|_| ClientError::Parent)?;
        if !root.directory || !root.disk || root.reparse {
            return Err(ClientError::Parent);
        }
        for (i, part) in names[..names.len() - 1].iter().enumerate() {
            if !path.ends_with('\\') {
                path.push('\\');
            }
            path.push_str(part);
            let final_parent = i + 2 == names.len();
            let handle = match open_dir(&path) {
                Ok(h) => h,
                Err(e) if e.kind() == io::ErrorKind::NotFound && final_parent => {
                    let sd = Descriptor::new(&user, true).map_err(|_| ClientError::Parent)?;
                    if unsafe { CreateDirectoryW(wide(&path).as_ptr(), &sd.attributes()) } == 0
                        && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
                    {
                        return Err(ClientError::Parent);
                    }
                    open_dir(&path).map_err(|_| ClientError::Parent)?
                }
                Err(_) => return Err(ClientError::Parent),
            };
            let f = facts(handle.as_handle()).map_err(|_| ClientError::Parent)?;
            if !f.directory || !f.disk || f.reparse {
                return Err(ClientError::Parent);
            }
            if final_parent {
                check(&f, &user, true, 0).map_err(|_| ClientError::Parent)?;
            }
            pins.push(handle);
        }
        check(
            &facts(pins.last().unwrap().as_handle()).map_err(|_| ClientError::Parent)?,
            &user,
            true,
            0,
        )
        .map_err(|_| ClientError::Parent)?;
        Ok(Self {
            _pins: pins,
            path: out.into(),
            user,
        })
    }
    pub(super) fn create(&self) -> Result<File, ClientError> {
        let sd = Descriptor::new(&self.user, false).map_err(|_| ClientError::Io)?;
        let h = unsafe {
            CreateFileW(
                wide(&self.path).as_ptr(),
                GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
                0,
                &sd.attributes(),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            let code = unsafe { GetLastError() };
            return Err(
                if code == ERROR_FILE_EXISTS || code == ERROR_ALREADY_EXISTS {
                    ClientError::Exists
                } else {
                    ClientError::Io
                },
            );
        }
        Ok(unsafe { File::from_raw_handle(h) })
    }
}
pub(super) struct NativeFileOps {
    user: String,
}
impl NativeFileOps {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            user: user(&token()?)?,
        })
    }
    pub(super) fn result_with(call: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        call()?;
        Ok(())
    }
    pub(super) fn verify_with(
        &self,
        acquire: impl FnOnce() -> io::Result<Facts>,
        expected: u64,
    ) -> io::Result<()> {
        let f = acquire()?;
        check(&f, &self.user, false, expected)
    }
}
impl PostCreateOps for NativeFileOps {
    fn write(&mut self, f: ClientHandle<'_>, json: &[u8]) -> io::Result<()> {
        Self::result_with(|| {
            let mut file =
                std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(f.as_raw_handle()) });
            file.write_all(json)
        })
    }
    fn flush(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        Self::result_with(|| bool_result(unsafe { FlushFileBuffers(f.as_raw_handle()) }))
    }
    fn verify(&mut self, f: ClientHandle<'_>, expected: u64) -> io::Result<()> {
        self.verify_with(|| facts(f), expected)
    }
}
#[cfg(test)]
impl NativeFileOps {
    // Synthetic handle facts only. The adapter still holds genuine current-user
    // comparison facts queried from TokenUser; no owner adapter is forged.
    pub(super) fn fixture_facts(&self, kind: u8, expected: u64) -> Facts {
        let mut f = Facts {
            owner: self.user.clone(),
            directory: false,
            disk: true,
            reparse: false,
            links: 1,
            length: expected,
            dacl: Some(vec![Ace {
                kind: 0,
                recipient: self.user.clone(),
            }]),
        };
        match kind {
            0 => {}
            1 => f.owner = "S-1-5-21-100-200-300-999".into(),
            2 => f.directory = true,
            3 => f.reparse = true,
            4 => f.links = 2,
            5 => f.length += 1,
            6 => f.dacl = None,
            7 => f.dacl.as_mut().unwrap().push(Ace {
                kind: 1,
                recipient: self.user.clone(),
            }),
            8 => f.dacl.as_mut().unwrap().push(Ace {
                kind: 9,
                recipient: self.user.clone(),
            }),
            9 => f.dacl.as_mut().unwrap().push(Ace {
                kind: 0,
                recipient: "S-1-5-21-100-200-300-999".into(),
            }),
            _ => f.disk = false,
        }
        f
    }
}
#[cfg(test)]
pub(super) fn fixture_public_parent(path: &str) -> io::Result<()> {
    let u = user(&token()?)?;
    let mut raw = ptr::null_mut();
    bool_result(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(&format!("O:{u}D:P(A;OICI;FA;;;{u})(A;OICI;FR;;;WD)")).as_ptr(),
            1,
            &mut raw,
            ptr::null_mut(),
        )
    })?;
    let sd = Descriptor(Local(raw));
    bool_result(unsafe { CreateDirectoryW(wide(path).as_ptr(), &sd.attributes()) })
}
#[cfg(test)]
pub(super) fn fixture_junction(path: &str, target: &str) -> io::Result<()> {
    bool_result(unsafe { CreateDirectoryW(wide(path).as_ptr(), ptr::null()) })?;
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let substitute = wide(&format!(r"\??\{target}"));
    let print = wide(target);
    let mut data = Vec::new();
    data.extend_from_slice(&0xA0000003u32.to_le_bytes());
    data.extend_from_slice(&(8u16 + ((substitute.len() + print.len()) * 2) as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    for value in [
        0u16,
        ((substitute.len() - 1) * 2) as u16,
        (substitute.len() * 2) as u16,
        ((print.len() - 1) * 2) as u16,
    ] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    for value in substitute.into_iter().chain(print) {
        data.extend_from_slice(&value.to_le_bytes());
    }
    let mut returned = 0;
    bool_result(unsafe {
        windows_sys::Win32::System::IO::DeviceIoControl(
            handle.as_raw_handle(),
            0x000900A4,
            data.as_ptr().cast(),
            data.len() as u32,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        )
    })
}

#[cfg(test)]
pub(super) fn fixture_snapshot(path: &str) -> io::Result<Facts> {
    facts(open_dir(path)?.as_handle())
}
