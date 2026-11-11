use anyhow::{Result, bail};
use std::path::Path;

#[cfg(unix)]
pub fn private(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    // Ownership checked against the foreground identity, never a configured execution identity.
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    if m.mode() & 0o077 != 0 || m.uid() != unsafe { geteuid() } {
        bail!("private owner-only permissions required");
    }
    Ok(())
}

#[cfg(windows)]
pub fn private(path: &Path) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    // Inspect the DACL in-process; do not spawn shell/security probes.
    unsafe {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = ptr::null_mut();
        let mut acl = ptr::null_mut();
        let status = GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            bail!("cannot inspect private permissions");
        }
        let result = (|| {
            if acl.is_null() {
                bail!("private DACL required");
            }
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                bail!("cannot inspect foreground identity");
            }
            let mut size = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
            let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            let ok =
                GetTokenInformation(token, TokenUser, data.as_mut_ptr().cast(), size, &mut size);
            CloseHandle(token);
            if ok == 0 {
                bail!("cannot inspect foreground identity");
            }
            let user = (*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid;
            for i in 0..(*acl).AceCount {
                let mut ace = ptr::null_mut();
                if GetAce(acl, i as u32, &mut ace) == 0 {
                    bail!("cannot inspect DACL entry");
                }
                let header = &*(ace.cast::<ACE_HEADER>());
                if header.AceFlags & 0x08 != 0 {
                    continue;
                } // inherit-only
                if header.AceType == 1 {
                    continue;
                } // deny ACE grants nothing
                if header.AceType != 0 {
                    bail!("unsupported private DACL entry");
                }
                let allow = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
                let sid = ptr::addr_of!(allow.SidStart) as *mut core::ffi::c_void;
                if EqualSid(sid, user) == 0
                    && IsWellKnownSid(sid, WinLocalSystemSid) == 0
                    && IsWellKnownSid(sid, WinBuiltinAdministratorsSid) == 0
                {
                    bail!("private owner-only permissions required");
                }
            }
            Ok(())
        })();
        LocalFree(descriptor);
        result
    }
}
