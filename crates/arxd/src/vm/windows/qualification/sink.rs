//! Diagnostic-only evidence output. The directory is held, privately owned
//! and claimed exclusively before any record is written. There is no file
//! name for a completed qualification in this contract version.

use super::*;
use std::cell::Cell;

/// The fixed diagnostic outputs. A completion marker does not exist here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticFile {
    Inputs,
    HostBaseline,
    Distribution,
    CustodyJobCapacity,
    ActivityReserves,
    WhpxQmpEndpoint,
    NetworkRestrictedTest,
    RecipeUbuntu,
    RecipeWindows,
    NetworkRestrictedOrigin,
    RecipeStaticWeb,
    NetworkPublicIpv4,
    RecipeDocker,
    Cleanup,
    /// The run summary. It is written last and closes the sink.
    Incomplete,
}

impl DiagnosticFile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Inputs => "inputs.json",
            Self::HostBaseline => "host-baseline.json",
            Self::Distribution => "distribution.json",
            Self::CustodyJobCapacity => "custody-job-capacity.json",
            Self::ActivityReserves => "activity-reserves.json",
            Self::WhpxQmpEndpoint => "whpx-qmp-endpoint.json",
            Self::NetworkRestrictedTest => "network-restricted-test.json",
            Self::RecipeUbuntu => "recipe-ubuntu.json",
            Self::RecipeWindows => "recipe-windows.json",
            Self::NetworkRestrictedOrigin => "network-restricted-origin.json",
            Self::RecipeStaticWeb => "recipe-static-web.json",
            Self::NetworkPublicIpv4 => "network-public-ipv4.json",
            Self::RecipeDocker => "recipe-docker.json",
            Self::Cleanup => "cleanup.json",
            Self::Incomplete => "incomplete.json",
        }
    }
}

const CLAIM: &str = "claim.json";
const CLAIM_BYTES: &[u8] = br#"{"claim":"arx-vm-qualify"}"#;
const MAX_OUTPUT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SinkError {
    Closed,
    Invalid,
    Private,
    Io,
}

pub(super) struct EvidenceDir {
    pin: PathPin,
    canaries: Vec<Vec<u8>>,
    closed: Cell<bool>,
}

impl EvidenceDir {
    /// Hold the directory, require private ownership and emptiness through
    /// the held object, then claim it with an exclusive create. A concurrent
    /// runner loses the claim and writes nothing.
    pub(super) fn claim(
        pin: PathPin,
        owner: PSID,
        private_values: &[&str],
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        if !held_private(&pin.leaf, owner) || !empty(&pin)? {
            return Err(reason);
        }
        let dir = Self {
            pin,
            canaries: private_values
                .iter()
                .filter(|value| value.len() >= 3)
                .flat_map(|value| {
                    let lower = value.to_ascii_lowercase();
                    [
                        lower.clone().into_bytes(),
                        lower.replace('\\', "\\\\").into_bytes(),
                        lower.replace('\\', "/").into_bytes(),
                    ]
                })
                .collect(),
            closed: Cell::new(false),
        };
        dir.create(CLAIM, CLAIM_BYTES).map_err(|_| reason)?;
        Ok(dir)
    }

    /// Exclusive, bounded, JSON-only and free of raw private values. The
    /// summary closes the sink so it is always the final file.
    pub(super) fn write(&self, file: DiagnosticFile, bytes: &[u8]) -> Result<(), SinkError> {
        if self.closed.get() {
            return Err(SinkError::Closed);
        }
        if !(1..=MAX_OUTPUT).contains(&bytes.len())
            || serde_json::from_slice::<serde::de::IgnoredAny>(bytes).is_err()
        {
            return Err(SinkError::Invalid);
        }
        if self.leaks(bytes) {
            return Err(SinkError::Private);
        }
        if file == DiagnosticFile::Incomplete {
            self.closed.set(true);
        }
        self.create(file.name(), bytes).map_err(|_| SinkError::Io)
    }

    fn leaks(&self, bytes: &[u8]) -> bool {
        let lower = bytes.to_ascii_lowercase();
        let contains = |needle: &[u8]| lower.windows(needle.len()).any(|w| w == needle);
        contains(b"s-1-")
            || contains(b":\\\\")
            || contains(b":/")
            || self.canaries.iter().any(|canary| contains(canary))
    }

    fn create(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let mut output = open_relative(&self.pin.leaf, name, Node::File, false, true)?;
        let written = output.write(bytes)?;
        if written != bytes.len() {
            return Err(io::Error::other("short evidence write"));
        }
        output.sync_all()
    }
}

fn empty(pin: &PathPin) -> Result<bool, NativeRefusal> {
    // The held directory and its held ancestors refuse rename, so the name
    // enumerated here is the pinned object.
    let mut items =
        std::fs::read_dir(pin.path.as_str()).map_err(|_| NativeRefusal::PathCustodyUnverified)?;
    Ok(items.next().is_none())
}

/// Owner and DACL of the held object itself: owned by the expected user,
/// with allow entries only for that user, SYSTEM and Administrators.
pub(super) fn held_private(file: &File, owner: PSID) -> bool {
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    let mut actual_owner = null_mut();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    // The descriptor owns the returned owner and DACL views.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut actual_owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return false;
    }
    // Windows-owned SID and ACL storage stays live until LocalFree below.
    let private = unsafe { private_descriptor(actual_owner, dacl, owner) };
    unsafe { LocalFree(descriptor) };
    private
}

/// The owner/DACL decision. All pointers must be null or valid for the call.
unsafe fn private_descriptor(actual_owner: PSID, dacl: *const ACL, owner: PSID) -> bool {
    // The caller keeps the SIDs and the ACL alive for this call.
    unsafe {
        !actual_owner.is_null()
            && EqualSid(actual_owner, owner) != 0
            && !dacl.is_null()
            && (0..u32::from((*dacl).AceCount)).all(|index| {
                let mut ace = null_mut();
                if GetAce(dacl, index, &mut ace) == 0 {
                    return false;
                }
                let header = &*ace.cast::<ACE_HEADER>();
                match header.AceType {
                    // A deny entry grants nothing.
                    1 => true,
                    0 => {
                        let allow = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                        let sid = std::ptr::addr_of!(allow.SidStart).cast_mut().cast();
                        EqualSid(sid, owner) != 0
                            || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
                    }
                    _ => false,
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;

    fn user() -> (Vec<usize>, String) {
        let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let data = token_data(&token, TokenUser).unwrap();
        let sid = unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text = null_mut();
        assert_ne!(
            unsafe {
                windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW(sid, &mut text)
            },
            0
        );
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let string = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).unwrap();
        unsafe { LocalFree(text.cast()) };
        (data, string)
    }

    fn sid(data: &[usize]) -> PSID {
        unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    /// A fresh directory with an explicit owner and protected DACL.
    fn directory(parent: &Path, name: &str, sddl: &str) -> LocalPath {
        let path = parent.join(name);
        let mut descriptor = null_mut();
        let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide_sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            0
        );
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let ok = unsafe { CreateDirectoryW(wide(&path).as_ptr(), &attributes) };
        unsafe { LocalFree(descriptor) };
        assert_ne!(ok, 0, "{}", io::Error::last_os_error());
        LocalPath::parse(path.to_str().unwrap()).unwrap()
    }

    fn private_dir(parent: &Path, name: &str) -> (Vec<usize>, LocalPath) {
        let (data, text) = user();
        let path = directory(parent, name, &format!("O:{text}D:P(A;OICI;FA;;;{text})"));
        (data, path)
    }

    fn names(path: &LocalPath) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path.as_str())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_private_empty_directory_is_claimed_once_and_closes_after_the_summary() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, path) = private_dir(root.path(), "evidence");
        let canary = r"D:\Private\Canary-Root";
        let dir = EvidenceDir::claim(
            PathPin::open(&path, true, false).unwrap(),
            sid(&owner),
            &[canary],
        )
        .unwrap();
        // A concurrent runner on the same directory loses the claim.
        assert_eq!(
            EvidenceDir::claim(PathPin::open(&path, true, false).unwrap(), sid(&owner), &[]).err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
        dir.write(DiagnosticFile::Inputs, br#"{"kind":"input_pins"}"#)
            .unwrap();
        // Exclusive creation refuses a collision with an existing record.
        assert_eq!(
            dir.write(DiagnosticFile::Inputs, br#"{"kind":"input_pins"}"#),
            Err(SinkError::Io)
        );
        for (bytes, error) in [
            (&b""[..], SinkError::Invalid),
            (&b"not json"[..], SinkError::Invalid),
            (&[b' '; MAX_OUTPUT + 1][..], SinkError::Invalid),
            (br#"{"owner":"S-1-5-21-1-2-3-1001"}"#, SinkError::Private),
            (
                br#"{"path":"d:\\private\\canary-root"}"#,
                SinkError::Private,
            ),
            (
                br#"{"path":"D:/Private/Canary-Root/x"}"#,
                SinkError::Private,
            ),
            (br#"{"path":"C:\\elsewhere"}"#, SinkError::Private),
        ] {
            assert_eq!(dir.write(DiagnosticFile::HostBaseline, bytes), Err(error));
        }
        dir.write(DiagnosticFile::Incomplete, br#"{"incomplete":true}"#)
            .unwrap();
        assert_eq!(
            dir.write(DiagnosticFile::Cleanup, br#"{"late":true}"#),
            Err(SinkError::Closed)
        );
        assert_eq!(
            names(&path),
            ["claim.json", "incomplete.json", "inputs.json"]
        );
    }

    #[test]
    fn the_descriptor_decision_checks_owner_and_every_grant() {
        use windows_sys::Win32::Security::{GetSecurityDescriptorDacl, GetSecurityDescriptorOwner};
        let (data, me) = user();
        let decide = |sddl: &str| {
            let mut descriptor = null_mut();
            let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        wide_sddl.as_ptr(),
                        1,
                        &mut descriptor,
                        null_mut(),
                    )
                },
                0,
                "{sddl}"
            );
            let (mut owner, mut defaulted, mut present, mut dacl) = (null_mut(), 0, 0, null_mut());
            unsafe {
                assert_ne!(
                    GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted),
                    0
                );
                assert_ne!(
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted),
                    0
                );
            }
            let decision = unsafe { private_descriptor(owner, dacl, sid(&data)) };
            unsafe { LocalFree(descriptor) };
            decision
        };
        for (name, sddl, private) in [
            ("owner only", format!("O:{me}D:P(A;OICI;FA;;;{me})"), true),
            (
                "owner, SYSTEM and Administrators",
                format!("O:{me}D:P(A;;FA;;;{me})(A;;FA;;;SY)(A;;FA;;;BA)"),
                true,
            ),
            (
                "deny entries grant nothing",
                format!("O:{me}D:P(D;;FA;;;WD)(A;;FA;;;{me})"),
                true,
            ),
            (
                "another owner",
                format!("O:S-1-5-21-1-2-3-1001D:P(A;;FA;;;{me})"),
                false,
            ),
            (
                "Administrators as owner",
                format!("O:BAD:P(A;;FA;;;{me})"),
                false,
            ),
            (
                "world readable",
                format!("O:{me}D:P(A;;FA;;;{me})(A;;FR;;;WD)"),
                false,
            ),
            (
                "authenticated users",
                format!("O:{me}D:P(A;;FA;;;{me})(A;;FR;;;AU)"),
                false,
            ),
            ("null DACL", format!("O:{me}D:NO_ACCESS_CONTROL"), false),
        ] {
            assert_eq!(decide(&sddl), private, "{name}");
        }
    }

    #[test]
    fn claim_refuses_nonprivate_nonempty_and_wrong_owner_directories() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, text) = user();
        let shared = directory(
            root.path(),
            "shared",
            &format!("O:{text}D:P(A;OICI;FA;;;{text})(A;OICI;FR;;;WD)"),
        );
        let (_, full) = private_dir(root.path(), "full");
        std::fs::write(format!(r"{}\stale.json", full.as_str()), b"{}").unwrap();
        for path in [&shared, &full] {
            assert_eq!(
                EvidenceDir::claim(PathPin::open(path, true, false).unwrap(), sid(&owner), &[])
                    .err(),
                Some(NativeRefusal::PathCustodyUnverified),
                "{}",
                path.as_str()
            );
        }
        // Ownership is compared against the expected owner, not just the DACL.
        let (_, private) = private_dir(root.path(), "private");
        let other = LocalSid::parse("S-1-5-21-1-2-3-1001").unwrap();
        assert_eq!(
            EvidenceDir::claim(PathPin::open(&private, true, false).unwrap(), other.0, &[]).err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
        assert!(names(&private).is_empty());
        assert!(names(&shared).is_empty());
    }
}
