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

/// The accepted DPAPI blob form: version 1, the default provider, and user
/// scope. `CRYPTPROTECT_LOCAL_MACHINE` blobs decrypt for every local account,
/// so a copied machine-scope file would not stay bound to the owner's login.
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn user_scope_dpapi(blob: &[u8]) -> bool {
    // {df9d8cd0-1501-11d1-8c7a-00c04fc297eb}, in its little-endian wire form.
    const PROVIDER: [u8; 16] = [
        0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2, 0x97,
        0xeb,
    ];
    const LOCAL_MACHINE: u32 = 0x4;
    // dwVersion, guidProvider, dwMasterKeyVersion, guidMasterKey, dwFlags.
    let word = |at: usize| {
        blob.get(at..at + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    word(0) == Some(1)
        && blob.get(4..20) == Some(PROVIDER.as_slice())
        && word(40).is_some_and(|flags| flags & LOCAL_MACHINE == 0)
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) fn unprotect_current_user(
    ciphertext: &mut [u8],
    max: usize,
) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        },
    };
    use zeroize::Zeroize;

    if !user_scope_dpapi(ciphertext) {
        bail!("publication credential unavailable");
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(ciphertext.len())?,
        pbData: ciphertext.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI allocates the output and transfers ownership on success.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            null_mut(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        bail!("publication credential unavailable");
    }
    // Bound the copy and clear the DPAPI allocation on every successful decrypt.
    unsafe {
        let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = if !bytes.is_empty() && bytes.len() <= max {
            Ok(zeroize::Zeroizing::new(bytes.to_vec()))
        } else {
            Err(anyhow::anyhow!("publication credential unavailable"))
        };
        bytes.zeroize();
        LocalFree(output.pbData.cast());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::user_scope_dpapi;

    fn header(version: u32, provider: [u8; 16], flags: u32) -> Vec<u8> {
        let mut blob = version.to_le_bytes().to_vec();
        blob.extend_from_slice(&provider);
        blob.extend_from_slice(&1u32.to_le_bytes());
        blob.extend_from_slice(&[7; 16]);
        blob.extend_from_slice(&flags.to_le_bytes());
        blob.extend_from_slice(&[0; 32]);
        blob
    }

    const PROVIDER: [u8; 16] = [
        0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2, 0x97,
        0xeb,
    ];

    #[test]
    fn dpapi_scope_accepts_only_user_scope_version_one_blobs() {
        assert!(user_scope_dpapi(&header(1, PROVIDER, 0x1)));
        assert!(!user_scope_dpapi(&header(1, PROVIDER, 0x4)));
        assert!(!user_scope_dpapi(&header(1, PROVIDER, 0x5)));
        assert!(!user_scope_dpapi(&header(2, PROVIDER, 0)));
        assert!(!user_scope_dpapi(&header(1, [0; 16], 0)));
        assert!(!user_scope_dpapi(&header(1, PROVIDER, 0)[..43]));
        assert!(!user_scope_dpapi(&[]));
    }

    /// Real DPAPI output under the current login, in both scopes.
    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn dpapi_scope_matches_real_protected_blobs() {
        use std::ptr::null;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
                CryptProtectData,
            },
        };
        let protect = |flags| {
            let mut value = b"synthetic scope probe".to_vec();
            let input = CRYPT_INTEGER_BLOB {
                cbData: value.len() as u32,
                pbData: value.as_mut_ptr(),
            };
            let mut output = CRYPT_INTEGER_BLOB::default();
            unsafe {
                assert_ne!(
                    CryptProtectData(&input, null(), null(), null(), null(), flags, &mut output),
                    0
                );
                let blob =
                    std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
                LocalFree(output.pbData.cast());
                blob
            }
        };
        let mut user = protect(CRYPTPROTECT_UI_FORBIDDEN);
        let mut machine = protect(CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE);
        assert!(user_scope_dpapi(&user));
        assert!(!user_scope_dpapi(&machine));
        assert_eq!(
            super::unprotect_current_user(&mut user, 64)
                .unwrap()
                .as_slice(),
            b"synthetic scope probe"
        );
        assert!(super::unprotect_current_user(&mut machine, 64).is_err());
    }
}
