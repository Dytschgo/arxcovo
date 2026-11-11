//! Read-only native observations. No owner operations or service entry wiring.
use super::{
    config::{Account, MAX_CONFIG_BYTES, Resolver, ServiceConfig},
    layout::{Error, Layout, Policy, Protection, check_protection},
};
use std::{fs::File, io::Read, path::Path};
#[cfg(unix)]
#[path = "native_unix.rs"]
mod os;
#[cfg(windows)]
#[path = "native_windows.rs"]
mod os;

#[cfg(target_os = "macos")]
#[path = "native_macos.rs"]
mod macos;

pub struct NativeResolver;
impl Resolver for NativeResolver {
    fn account(&self, value: &str) -> Result<Account, Error> {
        if value.is_empty() || value.len() > 256 || value.contains('\0') {
            return Err(Error::Identity);
        }
        os::account(value)
    }
    #[cfg(target_os = "macos")]
    fn revision(&self, value: &str) -> Result<Option<String>, Error> {
        macos::revision(value).map(Some)
    }
    fn workspace_safe(&self, value: &str) -> Result<bool, Error> {
        os::open(Path::new(value), true, false, false)
            .map(|_| true)
            .map_err(|_| Error::Workspace)
    }
}

/// A handle and its retained ancestry are kept alive together. Observations apply
/// to this opened object; they are not authority for a later path-based open.
pub struct Observed {
    file: File,
    _ancestors: Vec<File>,
    protection: Protection,
}
impl Observed {
    pub fn protection(&self) -> &Protection {
        &self.protection
    }
}

fn checked(path: &Path, directory: bool, policy: Policy, read: bool) -> Result<Observed, Error> {
    let observed = os::open(path, directory, read, true)?;
    check_protection(os::PLATFORM, policy, true, &observed.protection)?;
    Ok(observed)
}

/// Observe the fixed service layout. No arbitrary path override is exposed.
pub fn observe_layout(layout: &Layout) -> Result<Vec<Observed>, Error> {
    if layout.platform() != os::PLATFORM {
        return Err(Error::Layout);
    }
    let mut result = Vec::new();
    for path in layout.private_dirs() {
        result.push(checked(path, true, Policy::PrivateDirectory, false)?);
    }
    result.push(checked(
        layout.binary().parent().ok_or(Error::Layout)?,
        true,
        Policy::BinaryIntegrity,
        false,
    )?);
    result.push(checked(
        layout.binary(),
        false,
        Policy::BinaryIntegrity,
        false,
    )?);
    result.push(checked(layout.config(), false, Policy::PrivateFile, false)?);
    #[cfg(unix)]
    result.push(checked(
        layout.endpoint().parent().ok_or(Error::Layout)?,
        true,
        Policy::EndpointDirectory,
        false,
    )?);
    Ok(result)
}

/// Protection and bytes come from the same no-follow handle. Only the fixed
/// config is read. The existing parser performs fresh identity/pin checks.
pub fn read_config(layout: &Layout) -> Result<ServiceConfig, Error> {
    if layout.platform() != os::PLATFORM {
        return Err(Error::Layout);
    }
    let mut parents = Vec::new();
    for path in layout.private_dirs() {
        parents.push(checked(path, true, Policy::PrivateDirectory, false)?);
    }
    let mut opened = checked(layout.config(), false, Policy::PrivateFile, true)?;
    let bytes = bounded_read(&mut opened.file)?;
    // Reobserve the handle after reading, never reopen the symbolic path.
    check_protection(
        os::PLATFORM,
        Policy::PrivateFile,
        true,
        &os::protection(&opened.file)?,
    )?;
    ServiceConfig::parse_inventory(&bytes, layout, &NativeResolver)
}
#[cfg(unix)]
pub(crate) fn endpoint_guard(layout: &Layout) -> Result<Observed, Error> {
    checked(
        layout.endpoint().parent().ok_or(Error::Layout)?,
        true,
        Policy::EndpointDirectory,
        false,
    )
}
/// Service state is provisioned by the owner, never repaired or ACL-relaxed here.
/// SQLite uses a persistent TRUNCATE journal, so every mutable file can be
/// checked before the first write. Retain namespace pins for its entire life.
pub(crate) fn state_guard(layout: &Layout) -> Result<Vec<Observed>, Error> {
    require_service_identity()?;
    let mut pins = vec![checked(
        layout.state(),
        true,
        Policy::PrivateDirectory,
        false,
    )?];
    for name in [
        "activity.lock",
        "activity.sqlite3",
        "activity.sqlite3-journal",
    ] {
        let path = layout.state().join(name);
        #[cfg(windows)]
        let observed = os::open_state_file(&path)?;
        #[cfg(unix)]
        let observed = os::open(&path, false, false, true)?;
        check_protection(
            os::PLATFORM,
            Policy::PrivateFile,
            true,
            observed.protection(),
        )?;
        if name == "activity.sqlite3" {
            // SQLite may create WAL/SHM as soon as it opens a WAL-mode DB,
            // before our TRUNCATE pragma. Refuse that header before SQLite.
            let mut header = [0u8; 20];
            let mut file = &observed.file;
            let length = file.read(&mut header).map_err(|_| Error::Permissions)?;
            rollback_database_header(&header[..length])?;
        }
        pins.push(observed);
    }
    for name in ["activity.sqlite3-wal", "activity.sqlite3-shm"] {
        match std::fs::symlink_metadata(layout.state().join(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Error::Permissions),
        }
    }
    Ok(pins)
}
#[cfg(all(test, windows))]
pub(crate) fn state_fixture_pins(dir: &Path) -> anyhow::Result<Vec<Observed>> {
    let mut pins = vec![os::open_state_fixture(dir, true)?];
    for name in [
        "activity.lock",
        "activity.sqlite3",
        "activity.sqlite3-journal",
    ] {
        let path = dir.join(name);
        crate::security::private(&path)?;
        let pin = os::open_state_fixture(&path, false)?;
        let Protection::Windows { owner, .. } = pin.protection() else {
            anyhow::bail!("fixture platform");
        };
        anyhow::ensure!(*owner == crate::local::current_identity()?, "fixture owner");
        pins.push(pin);
    }
    Ok(pins)
}
fn rollback_database_header(header: &[u8]) -> Result<(), Error> {
    if header.is_empty()
        || (header.len() == 20 && &header[..16] == b"SQLite format 3\0" && header[18..20] == [1, 1])
    {
        Ok(())
    } else {
        Err(Error::Permissions)
    }
}
#[cfg(test)]
mod database_header_tests {
    use super::*;
    #[test]
    fn service_refuses_wal_before_sqlite_can_create_unprotected_companions() {
        assert_eq!(rollback_database_header(&[]), Ok(()));
        let mut header = [0u8; 20];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        header[18] = 1;
        header[19] = 1;
        assert_eq!(rollback_database_header(&header), Ok(()));
        for index in [18, 19] {
            header[index] = 2;
            assert_eq!(rollback_database_header(&header), Err(Error::Permissions));
            header[index] = 1;
        }
        assert_eq!(
            rollback_database_header(&header[..19]),
            Err(Error::Permissions)
        );
        header[0] = 0;
        assert_eq!(rollback_database_header(&header), Err(Error::Permissions));
    }
}
pub(crate) fn require_service_identity() -> Result<(), Error> {
    #[cfg(windows)]
    let valid = crate::local::current_identity().map_err(|_| Error::Permissions)? == "S-1-5-18";
    #[cfg(unix)]
    let valid = unsafe { libc::geteuid() == 0 && libc::getegid() == 0 };
    if valid {
        Ok(())
    } else {
        Err(Error::Permissions)
    }
}
fn bounded_read(file: &mut impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Config)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(Error::Config);
    }
    Ok(bytes)
}

/// Reject lexical aliases before the OS sees them. In particular Path::components
/// normalizes dots, so it must not be used as the validator.
fn components(path: &Path, windows: bool) -> Result<Vec<String>, Error> {
    let s = path.to_str().ok_or(Error::Layout)?;
    if s.len() > 4096 || s.contains('\0') {
        return Err(Error::Layout);
    }
    let normalized;
    let tail = if windows {
        normalized = s.replace('\\', "/");
        let b = normalized.as_bytes();
        if b.len() < 3 || !b[0].is_ascii_alphabetic() || &b[1..3] != b":/" {
            return Err(Error::Layout);
        }
        &normalized[3..]
    } else {
        if !s.starts_with('/') || s.contains('\\') {
            return Err(Error::Layout);
        }
        &s[1..]
    };
    let mut out = Vec::new();
    if tail.is_empty() {
        return Ok(out);
    }
    for c in tail.split('/') {
        if c.is_empty()
            || c == "."
            || c == ".."
            || c.chars().any(char::is_control)
            || (windows
                && (c.ends_with([' ', '.']) || c.contains([':', '<', '>', '"', '|', '?', '*'])))
        {
            return Err(Error::Layout);
        }
        if windows {
            let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
            if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|p| {
                    stem.strip_prefix(p).is_some_and(|n| {
                        matches!(
                            n,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                })
            {
                return Err(Error::Layout);
            }
        }
        out.push(c.to_owned());
    }
    if out.len() > 128 {
        return Err(Error::Layout);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_aliases_refuse_before_open() {
        for p in [
            "relative", "/a/../b", "/a/./b", "//a", "/a//b", "/a/", "/a\0b",
        ] {
            assert!(components(Path::new(p), false).is_err());
        }
        for p in [
            r"C:relative",
            r"\\host\share",
            r"\\?\C:\a",
            r"C:\a\..\b",
            r"C:\a:stream",
            r"C:\NUL.txt",
            r"C:\COM1",
            r"C:\a.",
        ] {
            assert!(components(Path::new(p), true).is_err());
        }
        assert!(components(Path::new(r"C:\ordinary\file"), true).is_ok());
    }
    #[test]
    fn reads_are_bounded_even_if_metadata_lies() {
        assert_eq!(
            bounded_read(&mut &vec![0; MAX_CONFIG_BYTES][..])
                .unwrap()
                .len(),
            MAX_CONFIG_BYTES
        );
        assert_eq!(
            bounded_read(&mut &vec![0; MAX_CONFIG_BYTES + 1][..]),
            Err(Error::Config)
        );
    }
    #[test]
    fn policy_negatives_cover_every_required_observation() {
        use super::super::layout::{Ace, Platform};
        let win = |owner: &str, protected, dacl_present, aces| Protection::Windows {
            owner: owner.into(),
            protected,
            dacl_present,
            aces,
        };
        for p in [
            win("S-1-5-32-544", true, true, vec![]),
            win("S-1-5-18", false, true, vec![]),
            win("S-1-5-18", true, false, vec![]),
            win("S-1-5-18", true, true, vec![Ace::Unsupported]),
            win(
                "S-1-5-18",
                true,
                true,
                vec![Ace::Allow {
                    sid: "S-1-1-0".into(),
                    mask: 0,
                    inherit_only: true,
                }],
            ),
        ] {
            assert_eq!(
                check_protection(Platform::Windows, Policy::PrivateFile, true, &p),
                Err(Error::Permissions)
            );
        }
        for (uid, gid, mode, safe) in [
            (1, 0, 0o600, true),
            (0, 1, 0o600, true),
            (0, 0, 0o644, true),
            (0, 0, 0o600, false),
        ] {
            assert_eq!(
                check_protection(
                    Platform::Linux,
                    Policy::PrivateFile,
                    safe,
                    &Protection::Unix { uid, gid, mode }
                ),
                Err(Error::Permissions)
            );
        }
    }
    #[test]
    fn rights_both_directions_and_pins_revalidate() {
        use super::super::{
            config::tests::{FakeResolver, fixture, parse},
            layout::Platform,
        };
        let mut v = fixture();
        v["rights"] = "admin".into();
        assert!(parse(v.clone(), &FakeResolver::default()).is_err());
        let c = parse(
            v,
            &FakeResolver {
                admin: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            c.revalidate(&Layout::for_test(Platform::Linux), &FakeResolver::default())
                .is_err()
        );
        let c = parse(fixture(), &FakeResolver::default()).unwrap();
        for r in [
            FakeResolver {
                admin: true,
                ..Default::default()
            },
            FakeResolver {
                changed: true,
                ..Default::default()
            },
            FakeResolver {
                safe: false,
                ..Default::default()
            },
            FakeResolver {
                group: true,
                ..Default::default()
            },
        ] {
            assert!(
                c.revalidate(&Layout::for_test(Platform::Linux), &r)
                    .is_err()
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn stock_system_directory_passes_trusted_ancestor_validation() {
        let path = std::path::PathBuf::from(
            std::env::var_os("SystemRoot").expect("Windows fixture requires SystemRoot"),
        )
        .join("System32");
        // System32 is a shared ancestor, not an Arxcovo final directory.
        let opened = os::open(&path, true, false, true)
            .unwrap_or_else(|error| panic!("stock system directory refused: {error:?}"));
        super::super::layout::check_windows_ancestor(opened.protection())
            .expect("System32 itself must be safe as a namespace ancestor");
        let layout = Layout::system().unwrap();
        for shared in [
            layout.binary().parent().unwrap().parent().unwrap(),
            layout
                .config()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap(),
        ] {
            let opened = os::open(shared, true, false, true)
                .expect("stock known-folder ancestor must open read-only");
            super::super::layout::check_windows_ancestor(opened.protection())
                .expect("stock known-folder namespace policy");
        }
    }
    #[test]
    fn native_workspace_rejects_missing_and_aliases() {
        let r = NativeResolver;
        assert!(r.workspace_safe("relative-secret-marker").is_err());
        let temp = tempfile::tempdir().unwrap();
        // macOS /var is a link: this intentionally refuses rather than canonicalizing.
        let result = r.workspace_safe(temp.path().to_str().unwrap());
        #[cfg(not(target_os = "macos"))]
        assert_eq!(result, Ok(true));
        #[cfg(target_os = "macos")]
        let _ = result;
        assert!(
            r.workspace_safe(temp.path().join("missing").to_str().unwrap())
                .is_err()
        );
        assert!(r.account("secret-marker\0").is_err());
    }
}
