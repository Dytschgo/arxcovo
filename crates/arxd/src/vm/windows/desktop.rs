use super::*;
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::{
    Security::Authorization::{GetSecurityInfo, SE_WINDOW_OBJECT},
    System::StationsAndDesktops::*,
};

const VISIBLE: u32 = 1; // WSF_VISIBLE
const OWNER_RIGHTS_SID: &str = "S-1-3-4";

// All standard-required and desktop-specific rights (0x000f_01ff).
const DESKTOP_ALL: u32 = 0x000f_01ff;
// Measured minimal desktop mask for a restricted QEMU-shaped child (0x0002_0083):
// - DESKTOP_READOBJECTS (0x0001): required by user32.dll init (removing fails 0xC0000142).
// - DESKTOP_CREATEWINDOW (0x0002): required by CreateDesktopW and CoInitialize (removing fails CoInitialize).
// - DESKTOP_WRITEOBJECTS (0x0080): required by user32.dll init (removing fails 0xC0000142).
// - READ_CONTROL (0x0002_0000): read-only descriptor inspection, matching the OWNER RIGHTS ceiling.
// Omits DESKTOP_HOOKCONTROL, DESKTOP_CREATEMENU, DESKTOP_JOURNAL*, DESKTOP_ENUMERATE,
// DESKTOP_SWITCHDESKTOP, DELETE, WRITE_DAC, and WRITE_OWNER.
const DESKTOP_STARTUP: u32 =
    READ_CONTROL | DESKTOP_READOBJECTS | DESKTOP_CREATEWINDOW | DESKTOP_WRITEOBJECTS;
// Launcher handle access on CreateDesktopW: CreateDesktopW requires DESKTOP_CREATEWINDOW
// internally, and ObjectSecurity::read requires READ_CONTROL.
const DESKTOP_CREATE_ACCESS: u32 = DESKTOP_CREATEWINDOW | READ_CONTROL;

// Window station specific access rights (winuser.h).
const WINSTA_ENUMDESKTOPS: u32 = 0x0001;
const WINSTA_READATTRIBUTES: u32 = 0x0002;
const WINSTA_ACCESSCLIPBOARD: u32 = 0x0004;
const WINSTA_CREATEDESKTOP: u32 = 0x0008;
const WINSTA_ACCESSGLOBALATOMS: u32 = 0x0020;
const WINSTA_ENUMERATE: u32 = 0x0100;
const WINSTA_ALL: u32 = 0x000f_037f;

// Launcher handle access on the logon station: create desktops, verify flags/name, read security.
const STATION_OPEN_ACCESS: u32 = WINSTA_CREATEDESKTOP | WINSTA_READATTRIBUTES | READ_CONTROL;
// First-create station account ACE: mirrors Windows' logon-station account grant (clipboard/atoms)
// plus READ_CONTROL; denies WINSTA_CREATEDESKTOP, WINSTA_WRITEATTRIBUTES, WINSTA_EXITWINDOWS,
// WINSTA_READSCREEN, DELETE, WRITE_DAC, and WRITE_OWNER to the restricted child.
const STATION_ACCOUNT: u32 = READ_CONTROL | WINSTA_ACCESSCLIPBOARD | WINSTA_ACCESSGLOBALATOMS;
// First-create station logon-SID ACE: grants the unrestricted launcher WINSTA_CREATEDESKTOP and
// read/enumerate rights without WRITE_DAC, WRITE_OWNER, DELETE, WINSTA_EXITWINDOWS, or WINSTA_READSCREEN.
const STATION_LOGON: u32 = READ_CONTROL
    | WINSTA_ENUMDESKTOPS
    | WINSTA_READATTRIBUTES
    | WINSTA_ACCESSCLIPBOARD
    | WINSTA_CREATEDESKTOP
    | WINSTA_ACCESSGLOBALATOMS
    | WINSTA_ENUMERATE;

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
        // Windows refuses CloseWindowStation while the handle is still assigned
        // as the process window station. After a terminal restore failure, the
        // health fence blocks further launches and handle recovery happens on
        // daemon process exit.
        if unsafe { CloseWindowStation(self.0) } == 0 {
            let error = io::Error::last_os_error();
            tracing::error!(
                os_error = error.raw_os_error(),
                "failed to release VM launch window station; recovery requires daemon exit if still assigned"
            );
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
        // Windows refuses CloseDesktop if any thread remains attached to it.
        // create_desktop never attaches the new desktop to a thread; if a
        // restore failure left a handle assigned, recovery happens on daemon exit.
        if unsafe { CloseDesktop(self.0) } == 0 {
            let error = io::Error::last_os_error();
            tracing::error!(
                os_error = error.raw_os_error(),
                "failed to release VM launch desktop; recovery requires daemon exit if still assigned"
            );
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
        let raw = unsafe { OpenWindowStationW(station_name.as_ptr(), 0, STATION_OPEN_ACCESS) };
        assert!(!raw.is_null(), "fixture station unavailable");
        let station = Station(raw);
        move || {
            // Capture the owning guard itself. Edition 2021 would otherwise
            // copy only the raw handle and close the station on return.
            let station = &station;
            // Launch desktops omit DESKTOP_ENUMERATE, so EnumDesktopsW skips
            // them. Probe presence directly via DESKTOP_READOBJECTS under the
            // same serialized station-switch guard used by create_desktop.
            let mut healthy = host_station().unwrap();
            let mut restore = Restore::capture(&mut healthy).unwrap();
            succeeded(unsafe { SetProcessWindowStation(station.0) }).unwrap();
            let raw = unsafe { OpenDesktopW(name.as_ptr(), 0, 0, DESKTOP_READOBJECTS) };
            let error = if raw.is_null() {
                Some(io::Error::last_os_error())
            } else {
                None
            };
            restore.apply(restore_original).unwrap();
            if !raw.is_null() {
                drop(Desktop(raw));
                true
            } else if error
                .as_ref()
                .is_some_and(|err| err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32))
            {
                false
            } else {
                panic!("unexpected desktop probe failure: {error:?}");
            }
        }
    }
}

/// Test custody of the shared logon station, held so an existing station cannot
/// be destroyed and recreated between reads of its security descriptor.
#[cfg(test)]
pub(super) struct StationWitness(Station);

#[cfg(test)]
impl StationWitness {
    /// Opens the logon-session station only if it already exists, so the
    /// witness never creates the station with a default descriptor ahead of
    /// the launcher's first-create descriptor.
    pub(super) fn open_existing(token: &VmToken) -> io::Result<Option<Self>> {
        let expected = wide(expected_station_name(token));
        let access = READ_CONTROL | WINSTA_READATTRIBUTES;
        let raw = unsafe { OpenWindowStationW(expected.as_ptr(), 0, access) };
        if raw.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) {
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some(Self(Station(raw))))
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

fn expected_station_name(token: &VmToken) -> String {
    let luid = token.identity.authentication;
    format!("Service-0x{:x}-{:x}$", luid.HighPart as u32, luid.LowPart)
}

fn desktop_entries(token: &VmToken, owner_rights: &LocalSid) -> [vm_acl::Entry; 4] {
    [
        vm_acl::Entry {
            deny: true,
            sid: token.network.0,
            mask: DESKTOP_ALL,
        },
        vm_acl::Entry {
            deny: false,
            sid: token.account.0,
            mask: DESKTOP_STARTUP,
        },
        // Cap implicit owner rights to READ_CONTROL so ownership by the
        // account SID cannot be used to obtain WRITE_DAC on launch desktops.
        vm_acl::Entry {
            deny: false,
            sid: owner_rights.0,
            mask: READ_CONTROL,
        },
        vm_acl::Entry {
            deny: false,
            sid: token.system.0,
            mask: DESKTOP_ALL,
        },
    ]
}

fn station_entries(token: &VmToken, owner_rights: &LocalSid) -> [vm_acl::Entry; 5] {
    [
        vm_acl::Entry {
            deny: true,
            sid: token.network.0,
            mask: WINSTA_ALL,
        },
        vm_acl::Entry {
            deny: false,
            sid: token.account.0,
            mask: STATION_ACCOUNT,
        },
        // Cap implicit owner rights to READ_CONTROL on first creation so
        // ownership cannot be used to rewrite the station's DACL.
        vm_acl::Entry {
            deny: false,
            sid: owner_rights.0,
            mask: READ_CONTROL,
        },
        vm_acl::Entry {
            deny: false,
            sid: token.identity.logon_sid(),
            mask: STATION_LOGON,
        },
        vm_acl::Entry {
            deny: false,
            sid: token.system.0,
            mask: WINSTA_ALL,
        },
    ]
}

fn build_descriptor(
    owner: PSID,
    entries: &[vm_acl::Entry],
    acl_storage: &mut [usize; 128],
    descriptor: &mut SECURITY_DESCRIPTOR,
) -> io::Result<SECURITY_ATTRIBUTES> {
    let acl = acl_storage.as_mut_ptr().cast::<ACL>();
    // Windows copies the descriptor during creation. No existing object's
    // security is modified by passing this descriptor to Create* APIs.
    unsafe {
        succeeded(InitializeAcl(
            acl,
            size_of_val(acl_storage) as u32,
            ACL_REVISION,
        ))?;
        for entry in entries {
            let append = if entry.deny {
                AddAccessDeniedAce
            } else {
                AddAccessAllowedAce
            };
            succeeded(append(acl, ACL_REVISION, entry.mask, entry.sid))?;
        }
        succeeded(InitializeSecurityDescriptor(ptr_of(descriptor), 1))?;
        succeeded(SetSecurityDescriptorOwner(ptr_of(descriptor), owner, 0))?;
        succeeded(SetSecurityDescriptorDacl(ptr_of(descriptor), 1, acl, 0))?;
        succeeded(SetSecurityDescriptorControl(
            ptr_of(descriptor),
            SE_DACL_PROTECTED,
            SE_DACL_PROTECTED,
        ))?;
    }
    Ok(SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr_of(descriptor),
        bInheritHandle: 0,
    })
}

impl LaunchDesktop {
    pub(super) fn new(token: &VmToken) -> io::Result<Self> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)
            .map_err(|_| io::Error::other("desktop name entropy unavailable"))?;
        let name = format!("Arxcovo-{}", hex::encode(random));
        let desktop_name = wide(&name);
        let owner_rights = LocalSid::parse(OWNER_RIGHTS_SID)?;

        // Station and desktop use distinct object-specific masks. The station
        // descriptor applies only when Windows creates the logon station on a
        // fresh session; an existing station's security is never modified.
        let station_acl_entries = station_entries(token, &owner_rights);
        let mut station_acl_storage = [0usize; 128];
        let mut station_sd = SECURITY_DESCRIPTOR::default();
        let station_attrs = build_descriptor(
            token.account.0,
            &station_acl_entries,
            &mut station_acl_storage,
            &mut station_sd,
        )?;

        let desktop_acl_entries = desktop_entries(token, &owner_rights);
        let mut desktop_acl_storage = [0usize; 128];
        let mut desktop_sd = SECURITY_DESCRIPTOR::default();
        let desktop_attrs = build_descriptor(
            token.account.0,
            &desktop_acl_entries,
            &mut desktop_acl_storage,
            &mut desktop_sd,
        )?;

        let (station, station_name) = logon_station(token, &station_attrs)?;
        let desktop = create_desktop(&station, &desktop_name, &desktop_attrs)?;
        verify_desktop(
            &ObjectSecurity::read(desktop.0)?,
            token.account.0,
            &desktop_acl_entries,
        )?;
        Ok(Self {
            path: wide(format!(r"{station_name}\{name}")),
            _desktop: desktop,
            _station: station,
        })
    }
}

fn logon_station(token: &VmToken, attrs: &SECURITY_ATTRIBUTES) -> io::Result<(Station, String)> {
    // The NULL form obtains the session station on both fresh and existing
    // logons. The descriptor is supplied for first creation only.
    let expected = expected_station_name(token);
    // Opening or creating the station does not switch the process station;
    // the health lock still refuses every launch after a failed restore.
    let station = {
        let _healthy = host_station()?;
        let handle = unsafe { CreateWindowStationW(null(), 0, STATION_OPEN_ACCESS, attrs) };
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
    let raw = unsafe {
        CreateDesktopW(
            name.as_ptr(),
            null(),
            null(),
            0,
            DESKTOP_CREATE_ACCESS,
            attributes,
        )
    };
    let result = if raw.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Desktop(raw))
    };
    // If restore fails, `result` drops while still unattached to any thread
    // so the desktop handle closes cleanly, the health fence refuses further
    // launches, and the still-assigned station handle recovers on daemon exit.
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
        assert_eq!(DESKTOP_STARTUP, 0x0002_0083);
        let me = LocalSid::parse("S-1-5-21-1-2-3-1001").unwrap();
        let owner_rights = LocalSid::parse(OWNER_RIGHTS_SID).unwrap();
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
                mask: DESKTOP_STARTUP,
            },
            vm_acl::Entry {
                deny: false,
                sid: owner_rights.0,
                mask: READ_CONTROL,
            },
            vm_acl::Entry {
                deny: false,
                sid: system.0,
                mask: DESKTOP_ALL,
            },
        ];
        let exact = "O:S-1-5-21-1-2-3-1001D:P(D;;0x000f01ff;;;NU)(A;;0x00020083;;;S-1-5-21-1-2-3-1001)(A;;0x00020000;;;OW)(A;;0x000f01ff;;;SY)";
        for (sddl, accept) in [
            (exact.to_owned(), true),
            (exact.replace("D:P", "D:"), false),
            (exact.replace("(D;;0x000f01ff;;;NU)", ""), false),
            (exact.replace("0x00020083", "0x000f01ff"), false),
            (exact.replace("(A;;0x00020000;;;OW)", ""), false),
            (
                exact.replace("(A;;0x00020000;;;OW)", "(A;;0x00060000;;;OW)"),
                false,
            ),
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
    fn first_create_station_descriptor_is_minimal_and_protected() {
        const WINSTA_WRITEATTRIBUTES: u32 = 0x0010;
        const WINSTA_EXITWINDOWS: u32 = 0x0040;
        const WINSTA_READSCREEN: u32 = 0x0200;

        assert_eq!(STATION_OPEN_ACCESS, 0x0002_000a);
        assert_eq!(STATION_ACCOUNT, 0x0002_0024);
        assert_eq!(STATION_LOGON, 0x0002_012f);

        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;

        let parent_primary = process_token(
            unsafe { GetCurrentProcess() },
            TOKEN_QUERY | TOKEN_DUPLICATE,
        )
        .unwrap();
        let user_data = token_data(&parent_primary, TokenUser).unwrap();
        let user_sid = unsafe { (*user_data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut sid_text = null_mut();
        succeeded(unsafe { ConvertSidToStringSidW(user_sid, &mut sid_text) }).unwrap();
        let sid = unsafe {
            let mut len = 0;
            while *sid_text.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, len));
            LocalFree(sid_text.cast());
            text
        };
        let account = LocalSid::parse(&sid).unwrap();
        let owner_rights = LocalSid::parse(OWNER_RIGHTS_SID).unwrap();
        let network = LocalSid::parse("S-1-5-2").unwrap();
        let system = LocalSid::parse("S-1-5-18").unwrap();
        let synthetic_logon = LocalSid::parse("S-1-5-5-0-42").unwrap();

        let (entries, token) = match VmToken::derive_current_user(LocalSid::parse(&sid).unwrap()) {
            Ok(token) => {
                let entries = station_entries(&token, &owner_rights);
                (entries, Some(token))
            }
            Err(_) => (
                [
                    vm_acl::Entry {
                        deny: true,
                        sid: network.0,
                        mask: WINSTA_ALL,
                    },
                    vm_acl::Entry {
                        deny: false,
                        sid: account.0,
                        mask: STATION_ACCOUNT,
                    },
                    vm_acl::Entry {
                        deny: false,
                        sid: owner_rights.0,
                        mask: READ_CONTROL,
                    },
                    vm_acl::Entry {
                        deny: false,
                        sid: synthetic_logon.0,
                        mask: STATION_LOGON,
                    },
                    vm_acl::Entry {
                        deny: false,
                        sid: system.0,
                        mask: WINSTA_ALL,
                    },
                ],
                None,
            ),
        };

        let owner_sid = token.as_ref().map_or(account.0, |t| t.account.0);
        let mut acl_storage = [0usize; 128];
        let mut sd = SECURITY_DESCRIPTOR::default();
        let attrs = build_descriptor(owner_sid, &entries, &mut acl_storage, &mut sd).unwrap();

        let mut owner = null_mut();
        let mut acl = null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        let mut control = 0;
        let mut revision = 0;
        unsafe {
            succeeded(GetSecurityDescriptorOwner(
                attrs.lpSecurityDescriptor,
                &mut owner,
                &mut defaulted,
            ))
            .unwrap();
            succeeded(GetSecurityDescriptorDacl(
                attrs.lpSecurityDescriptor,
                &mut present,
                &mut acl,
                &mut defaulted,
            ))
            .unwrap();
            succeeded(GetSecurityDescriptorControl(
                attrs.lpSecurityDescriptor,
                &mut control,
                &mut revision,
            ))
            .unwrap();
            assert_ne!(present, 0);
            assert_ne!(control & SE_DACL_PROTECTED, 0);
            assert_ne!(EqualSid(owner, owner_sid), 0);
            assert!(vm_acl::matches(acl, &entries));
        }

        // When running under the owner login, verify effective AccessCheck
        // against the production first-create station descriptor for both the
        // unrestricted launcher token and the restricted VM child token.
        let Some(token) = token else {
            return;
        };
        unsafe {
            // AccessCheck requires a group SID on the descriptor.
            succeeded(SetSecurityDescriptorGroup(ptr_of(&mut sd), owner_sid, 0)).unwrap();
        }
        let duplicate_imp = |src: HANDLE| -> OwnedHandle {
            let mut imp = null_mut();
            succeeded(unsafe { DuplicateToken(src, SecurityImpersonation, &mut imp) }).unwrap();
            unsafe { owned(imp) }.unwrap()
        };
        let parent_imp = duplicate_imp(parent_primary.as_raw_handle());
        let child_imp = duplicate_imp(token.token.as_raw_handle());

        let mapping = GENERIC_MAPPING {
            GenericRead: STANDARD_RIGHTS_READ
                | WINSTA_ENUMDESKTOPS
                | WINSTA_ENUMERATE
                | WINSTA_READATTRIBUTES
                | WINSTA_READSCREEN,
            GenericWrite: STANDARD_RIGHTS_WRITE
                | WINSTA_ACCESSCLIPBOARD
                | WINSTA_CREATEDESKTOP
                | WINSTA_WRITEATTRIBUTES,
            GenericExecute: STANDARD_RIGHTS_EXECUTE | WINSTA_ACCESSGLOBALATOMS | WINSTA_EXITWINDOWS,
            GenericAll: WINSTA_ALL,
        };
        let mut allowed = |imp: &OwnedHandle, desired: u32| -> bool {
            let mut privileges = PRIVILEGE_SET::default();
            let mut priv_len = size_of::<PRIVILEGE_SET>() as u32;
            let mut granted = 0;
            let mut status = 0;
            succeeded(unsafe {
                AccessCheck(
                    ptr_of(&mut sd),
                    imp.as_raw_handle(),
                    desired,
                    &mapping,
                    &mut privileges,
                    &mut priv_len,
                    &mut granted,
                    &mut status,
                )
            })
            .unwrap();
            status != 0
        };

        // Unrestricted launcher can open, create desktops, and enumerate.
        assert!(allowed(&parent_imp, STATION_OPEN_ACCESS));
        assert!(allowed(&parent_imp, WINSTA_ENUMDESKTOPS));
        // Neither launcher nor child gets DACL/owner/delete/exit/readscreen.
        for denied in [
            WRITE_DAC,
            WRITE_OWNER,
            DELETE,
            WINSTA_WRITEATTRIBUTES,
            WINSTA_EXITWINDOWS,
            WINSTA_READSCREEN,
        ] {
            assert!(!allowed(&parent_imp, denied), "parent allowed {denied:#x}");
            assert!(!allowed(&child_imp, denied), "child allowed {denied:#x}");
        }
        // Restricted child gets read/clipboard/atom rights only, never CREATEDESKTOP.
        assert!(allowed(
            &child_imp,
            READ_CONTROL
                | WINSTA_READATTRIBUTES
                | WINSTA_ENUMERATE
                | WINSTA_ACCESSCLIPBOARD
                | WINSTA_ACCESSGLOBALATOMS
        ));
        assert!(!allowed(&child_imp, WINSTA_CREATEDESKTOP));
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
