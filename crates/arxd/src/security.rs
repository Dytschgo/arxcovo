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

/// Match the actual handle's owner and the complete, ordered, explicit DACL.
/// The masks supplied here are object-specific rights, with no generic grants.
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) mod vm_acl {
    use std::{os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };

    pub(crate) struct Entry {
        pub deny: bool,
        pub sid: PSID,
        pub mask: u32,
    }

    /// All SID pointers must refer to valid SIDs throughout this call.
    pub(crate) unsafe fn exact(
        handle: &impl AsRawHandle,
        expected_owner: PSID,
        expected: &[Entry],
    ) -> bool {
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        // The descriptor owns the owner and DACL views.
        let status = unsafe {
            GetSecurityInfo(
                handle.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return false;
        }
        // GetSecurityInfo returned Windows-owned SID and ACL storage.
        let verified = unsafe {
            !owner.is_null() && EqualSid(owner, expected_owner) != 0 && matches(dacl, expected)
        };
        // Release the single allocation after inspecting its views.
        unsafe { LocalFree(descriptor) };
        verified
    }

    /// The ACL and expected SIDs must remain live for the call.
    pub(crate) unsafe fn matches(dacl: *const ACL, expected: &[Entry]) -> bool {
        // Only basic object-local ACEs in the intended order are recognized.
        unsafe {
            !dacl.is_null()
                && usize::from((*dacl).AceCount) == expected.len()
                && expected.iter().enumerate().all(|(index, entry)| {
                    let mut ace = ptr::null_mut();
                    if GetAce(dacl, index as u32, &mut ace) == 0 {
                        return false;
                    }
                    let header = &*ace.cast::<ACE_HEADER>();
                    if header.AceFlags != 0
                        || header.AceType != u8::from(entry.deny)
                        || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
                    {
                        return false;
                    }
                    // Basic allow and deny ACEs have the same mask and SID layout.
                    let basic = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                    let sid = ptr::addr_of!(basic.SidStart).cast_mut().cast();
                    basic.Mask == entry.mask && EqualSid(sid, entry.sid) != 0
                })
        }
    }
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
                // Inherit-only grants also expose files created in a private
                // directory, so validate their recipients before any creation.
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
