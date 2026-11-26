use super::*;
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::{
    Security::Authorization::{GetSecurityInfo, SE_WINDOW_OBJECT},
    System::StationsAndDesktops::*,
};

const VISIBLE: u32 = 1; // WSF_VISIBLE
const DESKTOP_ALL: u32 = 0x000f_01ff; // STANDARD_RIGHTS_REQUIRED and all desktop rights.
static STATION: Mutex<bool> = Mutex::new(true);

/// Last-input observation must not run during the temporary station switch.
pub(super) fn host_station() -> io::Result<MutexGuard<'static, bool>> {
    lock_healthy(&STATION)
}

fn lock_healthy(station: &Mutex<bool>) -> io::Result<MutexGuard<'_, bool>> {
    let guard = station
        .lock()
        .map_err(|_| io::Error::other("launch station lock poisoned"))?;
    if !*guard {
        return Err(io::Error::other("launch station restore failed"));
    }
    Ok(guard)
}

struct Station(HWINSTA);

// We own the process-local handle. Only the serialized operation below changes
// the process association, and only the final owner closes the handle.
unsafe impl Send for Station {}
unsafe impl Sync for Station {}

impl Drop for Station {
    fn drop(&mut self) {
        if unsafe { CloseWindowStation(self.0) } == 0 {
            tracing::error!("failed to release VM launch window station");
        }
    }
}

struct Desktop(HDESK);

// The held desktop is just a process-local handle. No call attaches a launch
// worker to it, and only the final owner closes the handle.
unsafe impl Send for Desktop {}
unsafe impl Sync for Desktop {}

impl Drop for Desktop {
    fn drop(&mut self) {
        if unsafe { CloseDesktop(self.0) } == 0 {
            tracing::error!("failed to release VM launch desktop");
        }
    }
}

/// Kept alongside the child and its Job until they settle. Field order
/// releases the desktop before the station.
pub(super) struct LaunchDesktop {
    pub(super) path: Vec<u16>,
    _desktop: Desktop,
    _station: Station,
}

#[cfg(test)]
impl LaunchDesktop {
    pub(super) fn closure_probe(&self) -> impl Fn() -> bool + use<> {
        let Ok((station_name, _)) = station_identity(self._station.0) else {
            panic!("fixture lost its station");
        };
        let station_name = wide(station_name);
        let path = self.path.clone();
        let name = path
            .rsplit(|&unit| unit == b'\\' as u16)
            .next()
            .unwrap()
            .to_vec();
        let raw = unsafe { OpenWindowStationW(station_name.as_ptr(), 0, GENERIC_ALL) };
        assert!(!raw.is_null(), "fixture station unavailable");
        let station = Station(raw);
        move || {
            // Capture the owning guard itself. Edition 2021 would otherwise
            // copy only the raw handle and close the station on return.
            let station = &station;
            let mut probe = (name.as_slice(), false);
            // EnumDesktops calls the callback synchronously, for NUL-terminated
            // names. Only compare the one expected fixture length.
            unsafe extern "system" fn found(
                peer: windows_sys::core::PCWSTR,
                context: LPARAM,
            ) -> i32 {
                let (name, exists) = unsafe { &mut *(context as *mut (&[u16], bool)) };
                for (index, unit) in name.iter().copied().enumerate() {
                    if unsafe { *peer.add(index) } != unit {
                        return 1;
                    }
                }
                *exists = true;
                1
            }
            succeeded(unsafe {
                EnumDesktopsW(station.0, Some(found), ptr_of(&mut probe) as LPARAM)
            })
            .unwrap();
            probe.1
        }
    }
}

/// Test custody of the shared logon station, held so the station cannot be
/// destroyed and recreated between reads of its security descriptor.
#[cfg(test)]
pub(super) struct StationWitness(Station);

#[cfg(test)]
impl StationWitness {
    pub(super) fn open() -> io::Result<Self> {
        // The NULL form names the caller's logon-session station, the same
        // session the derived VM token keeps. Read access only.
        const WINSTA_READATTRIBUTES: u32 = 0x0002;
        let access = READ_CONTROL | WINSTA_READATTRIBUTES;
        let raw = unsafe { CreateWindowStationW(null(), 0, access, null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(Station(raw)))
    }

    /// The owner and DACL as SDDL; the launcher must never change them.
    pub(super) fn security(&self) -> io::Result<String> {
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
        };
        let security = ObjectSecurity::read(self.0.0)?;
        let mut text = null_mut();
        let mut length = 0;
        succeeded(unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                security.descriptor,
                SDDL_REVISION_1,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut text,
                &mut length,
            )
        })?;
        // The returned buffer holds `length` UTF-16 units including the NUL.
        let units = unsafe { std::slice::from_raw_parts(text, length as usize) };
        let rendered = String::from_utf16_lossy(units.strip_suffix(&[0]).unwrap_or(units));
        unsafe { LocalFree(text.cast()) };
        Ok(rendered)
    }

    pub(super) fn identity(&self) -> io::Result<(String, u32)> {
        station_identity(self.0.0)
    }
}

impl LaunchDesktop {
    pub(super) fn new(token: &VmToken) -> io::Result<Self> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)
            .map_err(|_| io::Error::other("desktop name entropy unavailable"))?;
        let name = format!("Arxcovo-{}", hex::encode(random));
        let desktop_name = wide(&name);

        let entries = [
            vm_acl::Entry {
                deny: true,
                sid: token.network.0,
                mask: DESKTOP_ALL,
            },
            vm_acl::Entry {
                deny: false,
                sid: token.account.0,
                mask: DESKTOP_ALL,
            },
            vm_acl::Entry {
                deny: false,
                sid: token.system.0,
                mask: DESKTOP_ALL,
            },
        ];
        let mut acl_storage = [0usize; 128];
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        // Windows copies the descriptor during creation. No existing object's
        // security is set by this operation.
        unsafe {
            succeeded(InitializeAcl(
                acl,
                size_of_val(&acl_storage) as u32,
                ACL_REVISION,
            ))?;
            for entry in &entries {
                let append = if entry.deny {
                    AddAccessDeniedAce
                } else {
                    AddAccessAllowedAce
                };
                // Windows maps generic rights for the particular USER object.
                succeeded(append(acl, ACL_REVISION, GENERIC_ALL, entry.sid))?;
            }
            succeeded(InitializeSecurityDescriptor(ptr_of(&mut descriptor), 1))?;
            succeeded(SetSecurityDescriptorOwner(
                ptr_of(&mut descriptor),
                token.account.0,
                0,
            ))?;
            succeeded(SetSecurityDescriptorDacl(
                ptr_of(&mut descriptor),
                1,
                acl,
                0,
            ))?;
            succeeded(SetSecurityDescriptorControl(
                ptr_of(&mut descriptor),
                SE_DACL_PROTECTED,
                SE_DACL_PROTECTED,
            ))?;
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr_of(&mut descriptor),
            bInheritHandle: 0,
        };
        let (station, station_name) = logon_station(token, &attributes)?;
        let desktop = create_desktop(&station, &desktop_name, &attributes)?;
        verify_desktop(&ObjectSecurity::read(desktop.0)?, token.account.0, &entries)?;
        Ok(Self {
            path: wide(format!(r"{station_name}\{name}")),
            _desktop: desktop,
            _station: station,
        })
    }
}

fn logon_station(token: &VmToken, attrs: &SECURITY_ATTRIBUTES) -> io::Result<(Station, String)> {
    // The NULL form obtains the session station on both fresh and existing
    // logons. The descriptor is supplied for creation only.
    let luid = token.identity.authentication;
    let expected = format!("Service-0x{:x}-{:x}$", luid.HighPart as u32, luid.LowPart);
    // Opening or creating the station does not switch the process station;
    // the health lock still refuses every launch after a failed restore.
    let station = {
        let _healthy = host_station()?;
        let handle = unsafe { CreateWindowStationW(null(), 0, GENERIC_ALL, attrs) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Station(handle)
    };
    let (name, flags) = station_identity(station.0)?;
    check_station(&name, flags, &expected)?;
    let security = ObjectSecurity::read(station.0)?;
    let exact_owner = unsafe { EqualSid(security.owner, token.account.0) } != 0;
    // Record the DACL shape without logging the logon LUID or any SID.
    tracing::debug!(
        ace_count = unsafe { (*security.acl).AceCount },
        protected = security.protected,
        owner_matches = exact_owner,
        "verified non-visible VM logon station and inspected its DACL"
    );
    Ok((station, name))
}

fn check_station(name: &str, flags: u32, expected: &str) -> io::Result<()> {
    if flags & VISIBLE != 0
        || name.eq_ignore_ascii_case("WinSta0")
        || !name.eq_ignore_ascii_case(expected)
    {
        return Err(io::Error::other("unexpected or visible VM logon station"));
    }
    Ok(())
}

fn station_identity(station: HWINSTA) -> io::Result<(String, u32)> {
    let mut info = USEROBJECTFLAGS::default();
    let mut name = [0u16; 128];
    let mut used = 0;
    unsafe {
        succeeded(GetUserObjectInformationW(
            station,
            UOI_FLAGS,
            ptr_of(&mut info),
            size_of_val(&info) as u32,
            &mut used,
        ))?;
        succeeded(GetUserObjectInformationW(
            station,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            size_of_val(&name) as u32,
            &mut used,
        ))?;
    }
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| io::Error::other("unterminated window station name"))?;
    let name = String::from_utf16(&name[..end]).map_err(io::Error::other)?;
    Ok((name, info.dwFlags))
}

fn create_desktop(
    station: &Station,
    name: &[u16],
    attributes: &SECURITY_ATTRIBUTES,
) -> io::Result<Desktop> {
    let mut healthy = host_station()?;
    let mut restore = Restore::capture(&mut healthy)?;
    succeeded(unsafe { SetProcessWindowStation(station.0) })?;
    let raw = unsafe { CreateDesktopW(name.as_ptr(), null(), null(), 0, GENERIC_ALL, attributes) };
    let result = if raw.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Desktop(raw))
    };
    restore.apply(restore_original)?;
    result
}

struct Restore<'a> {
    original_station: HWINSTA,
    original_desktop: HDESK,
    healthy: &'a mut bool,
    pending: bool,
}

impl<'a> Restore<'a> {
    fn capture(healthy: &'a mut bool) -> io::Result<Self> {
        let original_station = unsafe { GetProcessWindowStation() };
        let original_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if original_station.is_null() || original_desktop.is_null() {
            return Err(io::Error::other(
                "launcher's original station or desktop unavailable",
            ));
        }
        Ok(Self {
            original_station,
            original_desktop,
            healthy,
            pending: true,
        })
    }

    fn apply(&mut self, restore: impl FnOnce(HWINSTA, HDESK) -> io::Result<()>) -> io::Result<()> {
        self.pending = false;
        restore(self.original_station, self.original_desktop).inspect_err(|_| {
            *self.healthy = false;
            tracing::error!("VM launch station restore failed; refusing further launches");
        })
    }
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        if self.pending {
            let _ = self.apply(restore_original);
        }
    }
}

fn restore_original(station: HWINSTA, desktop: HDESK) -> io::Result<()> {
    unsafe {
        if GetProcessWindowStation() != station {
            succeeded(SetProcessWindowStation(station))?;
        }
        if GetThreadDesktop(GetCurrentThreadId()) != desktop {
            succeeded(SetThreadDesktop(desktop))?;
        }
        if GetProcessWindowStation() != station || GetThreadDesktop(GetCurrentThreadId()) != desktop
        {
            return Err(io::Error::other("VM launch station did not restore"));
        }
    }
    Ok(())
}

struct ObjectSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    owner: PSID,
    acl: *mut ACL,
    protected: bool,
}

impl ObjectSecurity {
    fn read(handle: HANDLE) -> io::Result<Self> {
        let mut security = Self {
            descriptor: null_mut(),
            owner: null_mut(),
            acl: null_mut(),
            protected: false,
        };
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_WINDOW_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut security.owner,
                null_mut(),
                &mut security.acl,
                null_mut(),
                &mut security.descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if security.owner.is_null() || security.acl.is_null() || security.descriptor.is_null() {
            return Err(io::Error::other(
                "incomplete window object security descriptor",
            ));
        }
        let mut control = 0;
        let mut revision = 0;
        succeeded(unsafe {
            GetSecurityDescriptorControl(security.descriptor, &mut control, &mut revision)
        })?;
        security.protected = control & SE_DACL_PROTECTED != 0;
        Ok(security)
    }
}

impl Drop for ObjectSecurity {
    fn drop(&mut self) {
        unsafe { LocalFree(self.descriptor) };
    }
}

fn verify_desktop(
    security: &ObjectSecurity,
    owner: PSID,
    entries: &[vm_acl::Entry],
) -> io::Result<()> {
    if !security.protected
        || unsafe {
            EqualSid(security.owner, owner) == 0 || !vm_acl::matches(security.acl, entries)
        }
    {
        return Err(io::Error::other("VM desktop security descriptor mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;

    #[test]
    fn visible_or_foreign_station_is_refused() {
        let expected = "Service-0x0-abc$";
        assert!(check_station(expected, 0, expected).is_ok());
        assert!(check_station(expected, VISIBLE, expected).is_err());
        assert!(check_station("WinSta0", 0, "WinSta0").is_err());
        assert!(check_station("Service-0x0-def$", 0, expected).is_err());
    }

    #[test]
    fn desktop_requires_the_exact_protected_acl() {
        let me = LocalSid::parse("S-1-5-21-1-2-3-1001").unwrap();
        let system = LocalSid::parse("S-1-5-18").unwrap();
        let network = LocalSid::parse("S-1-5-2").unwrap();
        let expected = [
            vm_acl::Entry {
                deny: true,
                sid: network.0,
                mask: DESKTOP_ALL,
            },
            vm_acl::Entry {
                deny: false,
                sid: me.0,
                mask: DESKTOP_ALL,
            },
            vm_acl::Entry {
                deny: false,
                sid: system.0,
                mask: DESKTOP_ALL,
            },
        ];
        let exact = "O:S-1-5-21-1-2-3-1001D:P(D;;0x000f01ff;;;NU)(A;;0x000f01ff;;;S-1-5-21-1-2-3-1001)(A;;0x000f01ff;;;SY)";
        for (sddl, accept) in [
            (exact.to_owned(), true),
            (exact.replace("D:P", "D:"), false),
            (exact.replace("(D;;0x000f01ff;;;NU)", ""), false),
            (exact.replace("O:S-1-5-21-1-2-3-1001", "O:SY"), false),
        ] {
            let mut actual = ObjectSecurity {
                descriptor: null_mut(),
                owner: null_mut(),
                acl: null_mut(),
                protected: false,
            };
            let mut defaulted = 0;
            let mut present = 0;
            let mut control = 0;
            let mut revision = 0;
            unsafe {
                succeeded(ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(sddl).as_ptr(),
                    1,
                    &mut actual.descriptor,
                    null_mut(),
                ))
                .unwrap();
                succeeded(GetSecurityDescriptorOwner(
                    actual.descriptor,
                    &mut actual.owner,
                    &mut defaulted,
                ))
                .unwrap();
                succeeded(GetSecurityDescriptorDacl(
                    actual.descriptor,
                    &mut present,
                    &mut actual.acl,
                    &mut defaulted,
                ))
                .unwrap();
                succeeded(GetSecurityDescriptorControl(
                    actual.descriptor,
                    &mut control,
                    &mut revision,
                ))
                .unwrap();
            }
            actual.protected = control & SE_DACL_PROTECTED != 0;
            assert_eq!(verify_desktop(&actual, me.0, &expected).is_ok(), accept);
        }
    }

    #[test]
    fn failed_station_restore_refuses_later_switches() {
        let state = Mutex::new(true);
        {
            let mut healthy = lock_healthy(&state).unwrap();
            let mut restore = Restore {
                original_station: null_mut(),
                original_desktop: null_mut(),
                healthy: &mut healthy,
                pending: true,
            };
            assert!(
                restore
                    .apply(|_, _| Err(io::Error::other("fixture restore failure")))
                    .is_err()
            );
        }
        assert!(lock_healthy(&state).is_err());
    }
}
